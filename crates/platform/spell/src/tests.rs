use super::*;

fn words_of(text: &str, context: Context) -> Vec<&str> {
    words(text, &Region { range: 0..text.len(), context }).into_iter().map(|range| &text[range]).collect()
}

fn typos_of(text: &str, context: Context) -> Vec<String> {
    typos(english(), text, &[Region { range: 0..text.len(), context }]).into_iter().map(|typo| typo.word).collect()
}

#[test]
fn the_words_round_trip_through_the_bundled_format() {
    let encoded = encode(["Zebra", "apple", "apples", "apply", "apple", "Jean-Pierre", "don't"]);
    assert_eq!(decode(&encoded).unwrap(), ["apple", "apples", "apply", "don't", "zebra"]);
    assert!(decode(b"not deflate").is_err());
}

#[test]
fn the_bundled_dictionary_has_the_list_and_the_hand_written_words() {
    let english = english();
    assert!(english.len() > 230_000, "{}", english.len());
    assert!(english_if_loaded().is_some());
    for word in ["handler", "parse", "request", "dictionary", "zebra"] {
        assert!(english.contains(word), "{word}");
    }
    for word in extra_words() {
        assert!(english.contains(word), "{word} of extra.txt is in the dictionary");
    }
}

#[test]
fn regular_inflections_derivations_prefixes_and_compounds_are_known() {
    let english = english();
    for word in [
        "Handlers",
        "files",
        "classes",
        "parties",
        "parsed",
        "parsing",
        "running",
        "stopped",
        "copied",
        "returned",
        "bigger",
        "quickly",
        "simply",
        "happily",
        "tokenizer",
        "normalized",
        "configuration",
        "validation",
        "readable",
        "emptiness",
        "unmarshal",
        "reconnect",
        "preload",
        "nonempty",
        "filename",
        "keyword",
        "callbacks",
        "user's",
        "doesn't",
        "didn't",
        "can't",
        "we'll",
        "might've",
        "parser\u{2019}s",
        "colours",
        "behaviour",
        "centred",
        "initialise",
        "serialisation",
        "licence",
        "cancelled",
        "labelling",
        "became",
        "held",
    ] {
        assert!(english.knows(word), "{word} is known");
    }
}

#[test]
fn common_misspellings_are_not_known() {
    let english = english();
    for word in [
        "Hadnler",
        "recieve",
        "seperate",
        "adress",
        "lenght",
        "wich",
        "fucntion",
        "retrun",
        "stirng",
        "conifg",
        "widht",
        "heigth",
        "acess",
        "calback",
        "defualt",
        "paramter",
        "teh",
        "untill",
        "becuase",
        "enviroment",
        "wether",
        "alot",
        "thier",
        "beleive",
        "occurence",
        "definately",
        "wierd",
        "accomodate",
        "existance",
        "goverment",
        "neccessary",
        "succesful",
        "pubilc",
        "flase",
        "nubmer",
        "emtpy",
        "recursivly",
        "sucess",
        "dependancy",
        "refrence",
        "reponse",
        "similiar",
        "strenght",
        "valeu",
        "reutrn",
        "funciton",
    ] {
        assert!(!english.knows(word), "{word} is a typo");
    }
}

#[test]
fn names_split_at_capitals_underscores_and_digits() {
    assert_eq!(words_of("parseUrlHadnler", Context::Code), ["parse", "Url", "Hadnler"]);
    assert_eq!(typos_of("parseUrlHadnler", Context::Code), ["Hadnler"]);
    assert_eq!(words_of("HTTPServerConfig", Context::Code), ["Server", "Config"]);
    assert_eq!(words_of("max_retry_cuont", Context::Code), ["max", "retry", "cuont"]);
    assert_eq!(words_of("utf8Decoder2Handler", Context::Code), ["utf", "Decoder", "Handler"]);
    assert_eq!(typos_of("MAX_RETRY_CUONT", Context::Code), Vec::<String>::new(), "names in capitals are skipped");
}

#[test]
fn short_words_capitals_urls_emails_and_hex_numbers_are_skipped() {
    let comment = "// an id of OK HTTP JSON www.exampel.com https://exampel.org/hadnler mailto:x@y.z \
                   hadnler@exampel.com 0xDEADBEEF 0x1f #ff00aa 3fa9c0d2 deadbeef hadnler";
    assert_eq!(words_of(comment, Context::Comment), ["deadbeef", "hadnler"]);
    assert_eq!(typos_of(comment, Context::Comment), ["hadnler"], "deadbeef is dead and beef");
    assert_eq!(words_of("ab cd", Context::Comment), Vec::<&str>::new(), "two letters are too short");
    assert_eq!(words_of("teh", Context::Comment), ["teh"], "three letters are checked");
}

#[test]
fn literals_leave_out_escapes_and_format_verbs() {
    let literal = r#""\tparsing %dms of %08.3f, %v\nHadnler""#;
    assert_eq!(words_of(literal, Context::Literal), ["parsing", "Hadnler"]);
    assert_eq!(typos_of(literal, Context::Literal), ["Hadnler"]);
    assert_eq!(words_of(r"\tparsing", Context::Comment), ["tparsing"], "a comment has no escapes");
}

#[test]
fn words_outside_ascii_and_apostrophes_are_handled() {
    assert_eq!(words_of("// проверка naïve café Hadnler", Context::Comment), ["Hadnler"]);
    assert_eq!(words_of("'quoted' don't Hadnler's", Context::Text), ["quoted", "don't", "Hadnler"]);
    assert_eq!(typos_of("it doesn't wrok", Context::Text), ["wrok"]);
}

#[test]
fn typos_carry_their_place_and_context() {
    let text = "let parseUrlHadnler = \"recieve\"; // seperate";
    let regions = [
        Region { range: 4..19, context: Context::Code },
        Region { range: 22..31, context: Context::Literal },
        Region { range: 33..text.len(), context: Context::Comment },
    ];
    let found = typos(english(), text, &regions);
    let shown: Vec<(&str, Context)> = found.iter().map(|typo| (&text[typo.range.clone()], typo.context)).collect();
    assert_eq!(shown, [("Hadnler", Context::Code), ("recieve", Context::Literal), ("seperate", Context::Comment)]);
}

#[test]
fn a_region_off_the_text_or_inside_a_character_finds_nothing() {
    assert!(words("héllo", &Region { range: 2..6, context: Context::Text }).is_empty());
    assert_eq!(words("hello", &Region { range: 0..99, context: Context::Text }), vec![Range { start: 0, end: 5 }]);
}

#[test]
#[ignore = "needs /usr/share/dict/web2"]
fn the_bundled_file_is_reproduced_from_the_word_list() {
    let text = std::fs::read_to_string("/usr/share/dict/web2").unwrap();
    assert_eq!(encode(text.lines()), include_bytes!("../data/web2.z"));
}

#[test]
fn paths_dotted_names_colours_repeated_letters_and_words_against_digits_are_skipped() {
    let comment = "see docs/adr/0026-gogarden-first.md, ~/.zprofile, and/or fmt.Sprintf and egui::Context \
                   in space.ifzenelse.gogarden; #fff and #ff00aacc; aaaa zzz; macos-aarch64 GHSA-v778 hadnler";
    assert_eq!(words_of(comment, Context::Comment), ["see", "and", "and", "macos", "hadnler"]);
    assert_eq!(words_of("end of it. Next hadnler", Context::Text), ["end", "Next", "hadnler"]);
    assert_eq!(
        words_of("utf8Hadnler aarch64", Context::Code),
        ["utf", "Hadnler", "aarch"],
        "names still split at digits and check every piece"
    );
}

#[test]
fn british_and_american_spellings_are_both_known() {
    let english = english();
    for word in ["colour", "color", "serialisable", "serializable", "licence", "license", "centre", "center"] {
        assert!(english.knows(word), "{word} is known");
    }
    for word in ["xtask", "egui", "mistyped", "trackpad", "caffeinate", "dogfood", "ups"] {
        assert!(english.knows(word), "{word} is in the hand-written list");
    }
}

#[test]
fn suggestions_are_the_closest_words_in_the_typos_case() {
    let english = english();
    let first = |word: &str| suggestions(&[english], word, 5).into_iter().next().unwrap_or_default();
    assert_eq!(first("hadnler"), "handler");
    assert_eq!(first("Hadnler"), "Handler");
    assert_eq!(first("recieved"), "received", "an inflection of a listed word counts");
    assert_eq!(first("wrold"), "world");
    assert_eq!(first("adress"), "address");
    let listed = suggestions(&[english], "reqeust", 10);
    assert_eq!(listed.first().map(String::as_str), Some("request"), "{listed:?}");
    assert!(listed.len() <= 10);
    assert!(suggestions(&[english], "приветт", 5).is_empty(), "letters outside ASCII are not judged");
    let mine = Dictionary::from_words(["zenspell"]);
    assert_eq!(suggestions(&[english, &mine], "zenspel", 3).first().map(String::as_str), Some("zenspell"));
}

#[test]
fn typos_with_more_dictionaries_leave_out_the_words_any_of_them_knows() {
    let text = "the hadnler of zenspell";
    let region = [Region { range: 0..text.len(), context: Context::Comment }];
    let found = |dictionaries: &[&Dictionary]| -> Vec<String> {
        typos_with(dictionaries, text, &region).into_iter().map(|typo| typo.word).collect()
    };
    assert_eq!(found(&[english()]), ["hadnler", "zenspell"]);
    let mine = Dictionary::from_words(word_list("# mine\n\nZenSpell\n"));
    assert_eq!(found(&[english(), &mine]), ["hadnler"]);
    assert_eq!(word_list("\u{feff}one\r\n  two  \n#three\n\n"), ["one", "two"]);
}

const AFF: &str = "# A small hand-made dictionary.\nSET UTF-8\nTRY esianrtolcdugmphbyfvkwz\nNEEDAFFIX !\nFORBIDDENWORD *\n\nSFX S Y 2\nSFX S 0 s [^y]\nSFX S y ies [^aeiou]y\n\nSFX D N 3\nSFX D 0 ed [^ey]\nSFX D 0 d e\nSFX D y ied [^aeiou]y\n\nPFX U Y 1\nPFX U 0 un .\n\nPFX R N 1\nPFX R 0 re [^e]\n";

const DIC: &str = "6\nzorb/SDU\nfrobby/SD\nquux/R\nbloop/!S\nwrongword/*\nslash\\/ed\n";

#[test]
fn a_hunspell_dictionary_gives_its_stems_and_affixed_forms() {
    let mut forms = hunspell::words(AFF.as_bytes(), DIC.as_bytes()).unwrap();
    forms.sort();
    assert_eq!(
        forms,
        [
            "bloops", "frobbied", "frobbies", "frobby", "quux", "requux", "slash/ed", "unzorb", "unzorbs", "zorb",
            "zorbed", "zorbs",
        ],
        "S and U cross (unzorbs), D does not (no unzorbed); bloop needs an affix; wrongword is forbidden"
    );
    let dictionary = Dictionary::from_words(forms);
    assert!(dictionary.knows("Unzorbs") && dictionary.knows("frobbied") && !dictionary.knows("bloop"));
}

#[test]
fn hunspell_reads_long_and_numeric_flags_aliases_and_latin_1() {
    let aff = "FLAG long\nSFX Aa Y 1\nSFX Aa 0 er .\nPFX Bb Y 1\nPFX Bb 0 over .\n";
    let forms = hunspell::words(aff.as_bytes(), b"2\nrun/AaBb\nwalk/Aa\n").unwrap();
    for form in ["run", "runer", "overrun", "overruner", "walk", "walker"] {
        assert!(forms.iter().any(|known| known == form), "{form} in {forms:?}");
    }
    let aff = "FLAG num\nAF 2\nAF 7,9\nAF 9\nSFX 7 Y 1\nSFX 7 0 s .\nSFX 9 Y 1\nSFX 9 0 ing .\n";
    let forms = hunspell::words(aff.as_bytes(), b"2\njump/1\nsing/2\n").unwrap();
    assert_eq!(forms, ["jump", "jumps", "jumping", "sing", "singing"]);
    let latin = b"SET ISO8859-1\nSFX A Y 1\nSFX A 0 s .\n";
    let forms = hunspell::words(latin, b"1\ncaf\xe9/A\n").unwrap();
    assert_eq!(forms, ["caf\u{e9}", "caf\u{e9}s"]);
    assert!(hunspell::words(b"SFX\n", b"x\n").is_err(), "an affix without a flag");
}

#[test]
fn an_idea_dictionary_is_read_and_edited_in_place() {
    let file = "<component name=\"ProjectDictionaryState\">\n  <dictionary name=\"project\">\n    <words>\n      <w>gopls</w>\n      <w>zenide</w>\n    </words>\n  </dictionary>\n</component>";
    assert_eq!(idea_dictionary::words(file).unwrap(), ["gopls", "zenide"]);
    let added = idea_dictionary::edited(file, &["hadnler", "Gopls"], &[]).unwrap().unwrap();
    assert_eq!(
        added,
        "<component name=\"ProjectDictionaryState\">\n  <dictionary name=\"project\">\n    <words>\n      <w>gopls</w>\n      <w>hadnler</w>\n      <w>zenide</w>\n    </words>\n  </dictionary>\n</component>",
        "the new word goes in its place; a word already there in another case is not added again"
    );
    assert_eq!(idea_dictionary::edited(&added, &["gopls"], &["absent"]).unwrap(), None, "nothing changes");
    let removed = idea_dictionary::edited(&added, &[], &["HADNLER"]).unwrap().unwrap();
    assert_eq!(removed, file, "removed whatever its case, back to the bytes it was");
    let emptied = idea_dictionary::edited(file, &[], &["gopls", "zenide"]).unwrap().unwrap();
    assert_eq!(
        emptied,
        "<component name=\"ProjectDictionaryState\">\n  <dictionary name=\"project\">\n  </dictionary>\n</component>"
    );
    assert_eq!(idea_dictionary::edited(&emptied, &["gopls", "zenide"], &[]).unwrap().unwrap(), file);
    assert!(idea_dictionary::edited("<component/>", &["a"], &[]).is_err());
    assert!(idea_dictionary::words("<broken").is_err());
}

#[test]
fn an_idea_dictionary_keeps_what_it_does_not_know_about() {
    let file = "<?xml version=\"1.0\"?>\n<!-- kept -->\n<component name=\"ProjectDictionaryState\">\n\t<dictionary name=\"ada\" extra=\"1\"/>\n\t<other keep=\"yes\">x</other>\n</component>\n";
    let edited = idea_dictionary::edited(file, &["kubectl"], &[]).unwrap().unwrap();
    assert_eq!(
        edited,
        "<?xml version=\"1.0\"?>\n<!-- kept -->\n<component name=\"ProjectDictionaryState\">\n\t<dictionary name=\"ada\" extra=\"1\">\n\t  <words>\n\t    <w>kubectl</w>\n\t  </words>\n\t</dictionary>\n\t<other keep=\"yes\">x</other>\n</component>\n"
    );
    assert_eq!(idea_dictionary::words(&edited).unwrap(), ["kubectl"]);
    let two = "<component name=\"ProjectDictionaryState\"><dictionary name=\"a\"><words><w>one</w></words></dictionary><dictionary name=\"b\"><words><w>two</w><w>one</w></words></dictionary></component>";
    assert_eq!(idea_dictionary::words(two).unwrap(), ["one", "two", "one"]);
    let edited = idea_dictionary::edited(two, &[], &["one"]).unwrap().unwrap();
    assert_eq!(idea_dictionary::words(&edited).unwrap(), ["two"], "removed from every dictionary of the file");
    let fresh = idea_dictionary::new_file("project", &["alpha".to_owned(), "beta".to_owned()]);
    assert_eq!(idea_dictionary::words(&fresh).unwrap(), ["alpha", "beta"]);
    assert!(fresh.starts_with(
        "<component name=\"ProjectDictionaryState\">\n  <dictionary name=\"project\">\n    <words>\n      <w>alpha</w>"
    ));
}
