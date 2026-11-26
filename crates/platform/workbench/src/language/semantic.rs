use std::path::PathBuf;

use lsp_types::{SemanticTokensLegend, SemanticTokensServerCapabilities};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_lsp::{Client, Edit, Pending};

use super::{Languages, Synced};
use crate::Workbench;

/// A token: the characters it covers and what it is.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Token {
    /// The first character (a character offset of the file's text).
    pub start: usize,
    /// The character after the last.
    pub end: usize,
    /// The protocol's token type: `namespace`, `type`, `function`, `method`, `variable`,
    /// `parameter`, `keyword`, `string`, `number`...
    pub kind: String,
    /// Its modifiers: `definition`, `readonly`, `defaultLibrary`...
    pub modifiers: Vec<String>,
}

#[derive(Default)]
pub(super) struct FileTokens {
    pub(super) tokens: Vec<Token>,
    pub(super) generation: u64,
    pending: Option<(u64, Pending)>,
    asked: Option<u64>,
}

fn legend(client: &Client) -> Option<SemanticTokensLegend> {
    match client.capabilities()?.semantic_tokens_provider.as_ref()? {
        SemanticTokensServerCapabilities::SemanticTokensOptions(options) => Some(options.legend.clone()),
        SemanticTokensServerCapabilities::SemanticTokensRegistrationOptions(options) => {
            Some(options.semantic_tokens_options.legend.clone())
        }
    }
}

pub(super) fn step(client: &mut Client, synced: &mut Synced) {
    let Some(legend) = legend(client) else { return };
    let tokens = &mut synced.tokens;
    if let Some((version, pending)) = &tokens.pending
        && let Some(answer) = pending.try_answer()
    {
        let version = *version;
        tokens.pending = None;
        if version == synced.version
            && let Ok(answer) = answer
            && let Some(data) = answer["data"].as_array()
        {
            let data: Vec<u32> = data.iter().filter_map(|value| value.as_u64().map(|value| value as u32)).collect();
            tokens.tokens = decode(&synced.text, &data, &legend);
            tokens.generation += 1;
        }
    }
    if tokens.pending.is_none() && tokens.asked != Some(synced.version) {
        let request =
            client.request("textDocument/semanticTokens/full", json!({ "textDocument": { "uri": synced.uri } }));
        if let Ok(pending) = request {
            tokens.pending = Some((synced.version, pending));
            tokens.asked = Some(synced.version);
        }
    }
}

pub(super) fn follow_edit(tokens: &mut FileTokens, old: &str, edit: &Edit) {
    let start = super::char_offset(old, edit.range.start);
    let end = super::char_offset(old, edit.range.end);
    let delta = edit.text.chars().count() as isize - (end - start) as isize;
    tokens.tokens.retain_mut(|token| {
        if token.end <= start {
            true
        } else if token.start >= end {
            token.start = (token.start as isize + delta) as usize;
            token.end = (token.end as isize + delta) as usize;
            true
        } else {
            false
        }
    });
    tokens.generation += 1;
}

fn decode(text: &str, data: &[u32], legend: &SemanticTokensLegend) -> Vec<Token> {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut line_starts = Vec::with_capacity(lines.len());
    let mut offset = 0;
    for line in &lines {
        line_starts.push(offset);
        offset += line.chars().count() + 1;
    }
    let column = |line: &str, utf16: u32| -> usize {
        let mut units = 0;
        for (index, c) in line.chars().enumerate() {
            if units >= utf16 as usize {
                return index;
            }
            units += c.len_utf16();
        }
        line.chars().count()
    };
    let mut tokens = Vec::with_capacity(data.len() / 5);
    let (mut line, mut start) = (0u32, 0u32);
    for &[delta_line, delta_start, length, kind, modifiers] in data.as_chunks::<5>().0 {
        line += delta_line;
        start = if delta_line == 0 { start + delta_start } else { delta_start };
        let Some(text_line) = lines.get(line as usize) else { break };
        let from = column(text_line, start);
        let to = column(text_line, start + length);
        let base = line_starts[line as usize];
        let kind = legend.token_types.get(kind as usize).map_or("", |kind| kind.as_str()).to_owned();
        let modifiers = legend
            .token_modifiers
            .iter()
            .enumerate()
            .filter(|(bit, _)| modifiers & (1 << bit) != 0)
            .map(|(_, modifier)| modifier.as_str().to_owned())
            .collect();
        tokens.push(Token { start: base + from, end: base + to, kind, modifiers });
    }
    tokens
}

/// The parameters of `editor.semantic.tokens`.
#[derive(Default, Deserialize, JsonSchema)]
pub struct TokensParams {
    /// The file; the active one when absent.
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// The generation the caller has; when it is still current, the tokens are left out.
    #[serde(default)]
    pub since: Option<u64>,
}

/// What `editor.semantic.tokens` answers.
#[derive(Serialize, JsonSchema)]
pub struct TokensAnswer {
    /// Bumped whenever the tokens change.
    pub generation: u64,
    /// The tokens, unless `since` was current.
    pub tokens: Option<Vec<Token>>,
}

pub struct SemanticTokens;

impl Command for SemanticTokens {
    type Params = TokensParams;
    type Output = TokensAnswer;
    const ID: &'static str = "editor.semantic.tokens";
    const TITLE: &'static str = "Semantic Tokens";
    const DESCRIPTION: &'static str = "What each name in a file is, from the language server (packages, types, functions...), \
         as character ranges; the active file by default.";

    fn run(&self, session: &mut Session, params: TokensParams) -> Result<TokensAnswer, CommandError> {
        let path = params
            .path
            .or_else(|| {
                session
                    .state::<Workbench>()
                    .and_then(Workbench::active_document)
                    .map(|document| document.path().to_owned())
            })
            .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
        let Some(languages) = session.state::<Languages>() else {
            return Ok(TokensAnswer { generation: 0, tokens: Some(Vec::new()) });
        };
        let found = languages.servers.values().find_map(|server| match server {
            super::Server::Running { files, .. } => files.get(&path).map(|synced| &synced.tokens),
            _ => None,
        });
        Ok(match found {
            Some(tokens) if params.since == Some(tokens.generation) => {
                TokensAnswer { generation: tokens.generation, tokens: None }
            }
            Some(tokens) => TokensAnswer { generation: tokens.generation, tokens: Some(tokens.tokens.clone()) },
            None => TokensAnswer { generation: 0, tokens: Some(Vec::new()) },
        })
    }
}

fn file_tokens<'a>(session: &'a Session, path: &std::path::Path) -> Option<&'a FileTokens> {
    let languages = session.state::<Languages>()?;
    let tokens = languages.servers.values().find_map(|server| match server {
        super::Server::Running { files, .. } => files.get(path).map(|synced| &synced.tokens),
        _ => None,
    })?;
    (tokens.generation != 0).then_some(tokens)
}

pub(crate) fn declarations_generation(session: &Session, path: &std::path::Path) -> Option<u64> {
    file_tokens(session, path).map(|tokens| tokens.generation)
}

pub(crate) fn declarations(session: &Session, path: &std::path::Path) -> Option<Vec<(usize, usize)>> {
    let tokens = file_tokens(session, path)?;
    Some(
        tokens
            .tokens
            .iter()
            .filter(|token| {
                token.modifiers.iter().any(|modifier| modifier == "definition" || modifier == "declaration")
            })
            .map(|token| (token.start, token.end))
            .collect(),
    )
}

pub(super) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(SemanticTokens)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsp_types::{SemanticTokenModifier, SemanticTokenType};

    fn legend() -> SemanticTokensLegend {
        SemanticTokensLegend {
            token_types: vec![SemanticTokenType::NAMESPACE, SemanticTokenType::TYPE],
            token_modifiers: vec![SemanticTokenModifier::DEFINITION, SemanticTokenModifier::DEFAULT_LIBRARY],
        }
    }

    #[test]
    fn tokens_are_decoded_into_characters() {
        let text = "// привет\nvar _ fmt.Stringer = x\n";
        let data = [1, 6, 3, 0, 0, 0, 4, 8, 1, 0b10];
        let tokens = decode(text, &data, &legend());
        let chars: Vec<char> = text.chars().collect();
        let word = |token: &Token| chars[token.start..token.end].iter().collect::<String>();
        assert_eq!(word(&tokens[0]), "fmt");
        assert_eq!(tokens[0].kind, "namespace");
        assert_eq!(word(&tokens[1]), "Stringer");
        assert_eq!(tokens[1].kind, "type");
        assert_eq!(tokens[1].modifiers, ["defaultLibrary"]);
    }

    #[test]
    fn tokens_move_with_edits() {
        let old = "a fmt b";
        let mut tokens = FileTokens {
            tokens: vec![
                Token { start: 2, end: 5, kind: "namespace".into(), modifiers: Vec::new() },
                Token { start: 6, end: 7, kind: "type".into(), modifiers: Vec::new() },
            ],
            ..FileTokens::default()
        };
        follow_edit(&mut tokens, old, &Edit { range: 2..2, text: "xx".into() });
        assert_eq!((tokens.tokens[0].start, tokens.tokens[1].start), (4, 8));
        follow_edit(&mut tokens, "a xxfmt b", &Edit { range: 5..6, text: String::new() });
        assert_eq!(tokens.tokens.len(), 1);
    }
}
