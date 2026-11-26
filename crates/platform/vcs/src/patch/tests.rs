use proptest::prelude::*;

use super::*;

const FORMAT_PATCH: &str = "From 18d0235be607b0336c0c12441e8a1cf81d34697c Mon Sep 17 00:00:00 2001
From: =?UTF-8?q?Ada_L=C3=B6velace?= <ada@example.com>
Date: Thu, 22 Oct 2026 06:05:01 +0800
Subject: [PATCH] Fix the greeting
 across two lines

The body says why.

Second paragraph.
---
 a.txt | 2 +-
 1 file changed, 1 insertion(+), 1 deletion(-)

diff --git a/a.txt b/a.txt
index 4d1ae35..a2a5cf2 100644
--- a/a.txt
+++ b/a.txt
@@ -1,3 +1,3 @@ fn main
 one
-two
+TWO
 three
diff --git a/old name.txt b/new name.txt
similarity index 100%
rename from old name.txt
rename to new name.txt
diff --git a/run.sh b/run.sh
old mode 100644
new mode 100755
diff --git a/gone.txt b/gone.txt
deleted file mode 100644
index 3be9c81..0000000
--- a/gone.txt
+++ /dev/null
@@ -1 +0,0 @@
-gone
diff --git a/new.txt b/new.txt
new file mode 100644
index 0000000..b1d1f3a
--- /dev/null
+++ b/new.txt
@@ -0,0 +1,2 @@
+first
+last
\\ No newline at end of file
--
2.50.1
";

#[test]
fn a_format_patch_is_read_with_its_mail() {
    let patch = parse(FORMAT_PATCH.as_bytes()).unwrap();
    let mail = patch.mail.clone().unwrap();
    assert_eq!(mail.commit.as_deref(), Some("18d0235be607b0336c0c12441e8a1cf81d34697c"));
    assert_eq!(mail.author, "Ada Lövelace <ada@example.com>");
    assert_eq!(mail.subject, "Fix the greeting across two lines");
    assert_eq!(mail.body, "The body says why.\n\nSecond paragraph.");
    assert_eq!(mail.message(), "Fix the greeting across two lines\n\nThe body says why.\n\nSecond paragraph.");
    let kinds: Vec<FileKind> = patch.files.iter().map(|file| file.kind).collect();
    assert_eq!(kinds, [FileKind::Modified, FileKind::Renamed, FileKind::Modified, FileKind::Deleted, FileKind::Added]);
    assert_eq!(patch.files[0].hunks[0].section, b" fn main");
    assert_eq!(patch.files[0].old_blob(), Some("4d1ae35"));
    assert_eq!(patch.files[1].old_path.as_deref(), Some("a/old name.txt"));
    assert_eq!(patch.files[1].new_path.as_deref(), Some("b/new name.txt"));
    assert_eq!(patch.files[2].new_mode.as_deref(), Some("100755"));
    assert_eq!(patch.files[3].new_path, None);
    assert_eq!(patch.files[4].old_path, None);
    assert_eq!(patch.files[4].old_blob(), None);
    assert_eq!(patch.files[4].hunks[0].lines[1].text, b"last", "no line break at the end");
    assert_eq!(patch.detected_strip(), 1);
}

#[test]
fn a_patch_written_again_reads_the_same() {
    let patch = parse(FORMAT_PATCH.as_bytes()).unwrap().strip(1).unwrap();
    let written = patch.to_bytes();
    let again = parse(&written).unwrap().strip(1).unwrap();
    assert_eq!(again, patch, "{}", String::from_utf8_lossy(&written));
    let text = String::from_utf8(written).unwrap();
    assert!(text.starts_with("From 18d0235be607b0336c0c12441e8a1cf81d34697c Mon Sep 17 00:00:00 2001\n"), "{text}");
    assert!(text.contains("From: =?UTF-8?q?Ada_L=C3=B6velace?= <ada@example.com>\n"), "{text}");
    assert!(text.contains("Subject: [PATCH] Fix the greeting across two lines\n"), "{text}");
    assert!(
        text.contains("diff --git a/old name.txt b/new name.txt\nsimilarity index 100%\nrename from old name.txt\n")
    );
    assert!(text.contains("+last\n\\ No newline at end of file\n"), "{text}");
}

#[test]
fn a_plain_unified_diff_is_read() {
    let text = "Index: src/a.c\n===================\n--- src/a.c\t2026-10-22 10:00:00.000000000 +0300\n\
                +++ src/a.c\t2026-10-22 10:01:00.000000000 +0300\n@@ -2 +2 @@\n-b\n+B\n";
    let patch = parse(text.as_bytes()).unwrap();
    assert_eq!(patch.mail, None);
    assert_eq!(patch.files[0].old_path.as_deref(), Some("src/a.c"));
    assert_eq!(patch.detected_strip(), 0);
    assert_eq!((patch.files[0].hunks[0].old_start, patch.files[0].hunks[0].old_lines), (2, 1));
}

#[test]
fn quoted_paths_are_unquoted_and_quoted_back() {
    let text = "diff --git \"a/caf\\303\\251 \\\"x\\\".txt\" \"b/caf\\303\\251 \\\"x\\\".txt\"\nindex 1..2 100644\n\
                --- \"a/caf\\303\\251 \\\"x\\\".txt\"\n+++ \"b/caf\\303\\251 \\\"x\\\".txt\"\n@@ -1 +1 @@\n-a\n+b\n";
    let patch = parse(text.as_bytes()).unwrap().strip(1).unwrap();
    assert_eq!(patch.files[0].path(), "café \"x\".txt");
    let written = String::from_utf8(patch.to_bytes()).unwrap();
    assert!(written.starts_with("diff --git \"a/café \\\"x\\\".txt\" \"b/café \\\"x\\\".txt\"\n"), "{written}");
    assert_eq!(parse(written.as_bytes()).unwrap().strip(1).unwrap(), patch);
}

#[test]
fn binary_data_is_kept_and_reversed_by_swapping_its_blocks() {
    let text = "diff --git a/b.bin b/b.bin\nindex 1..2 100644\nGIT binary patch\nliteral 3\nKcmZQzWB>*00ICy\n\n\
                literal 4\nLcmZQzWMT#Y01f~L\n\ndiff --git a/c.txt b/c.txt\n--- a/c.txt\n+++ b/c.txt\n@@ -1 +1 @@\n-c\n+C\n";
    let patch = parse(text.as_bytes()).unwrap();
    assert_eq!(patch.files.len(), 2);
    let Some(Binary::Data(data)) = &patch.files[0].binary else { panic!("{:?}", patch.files[0]) };
    assert_eq!(data[0], b"literal 3");
    assert_eq!(data.len(), 5);
    let reversed = patch.reversed();
    let Some(Binary::Data(data)) = &reversed.files[0].binary else { panic!() };
    assert_eq!(data[0], b"literal 4");
    assert_eq!(reversed.reversed(), patch);
    let written = String::from_utf8(patch.strip(1).unwrap().to_bytes()).unwrap();
    assert!(written.contains("GIT binary patch\nliteral 3\nKcmZQzWB>*00ICy\n\nliteral 4\n"), "{written}");
}

#[test]
fn a_reversed_patch_undoes_it() {
    let patch = parse(FORMAT_PATCH.as_bytes()).unwrap().strip(1).unwrap();
    let reversed = patch.reversed();
    assert_eq!(reversed.files[3].kind, FileKind::Added);
    assert_eq!(reversed.files[4].kind, FileKind::Deleted);
    assert_eq!(reversed.files[1].old_path.as_deref(), Some("new name.txt"));
    assert_eq!(reversed.files[2].old_mode.as_deref(), Some("100755"));
    assert_eq!(reversed.files[0].index.as_deref(), Some("a2a5cf2..4d1ae35"));
    let original = b"one\ntwo\nthree\n";
    let changed = apply(original, &patch.files[0]);
    assert!(changed.clean());
    assert_eq!(changed.theirs, b"one\nTWO\nthree\n");
    let back = apply(&changed.theirs, &reversed.files[0]);
    assert_eq!(back.theirs, original);
    assert_eq!(reversed.reversed(), patch);
}

fn file(text: &str) -> FilePatch {
    parse(text.as_bytes()).unwrap().strip(1).unwrap().files.remove(0)
}

#[test]
fn hunks_apply_where_their_lines_moved() {
    let patch = file("--- a/f\n+++ b/f\n@@ -2,3 +2,3 @@\n b\n-c\n+C\n d\n@@ -8,2 +8,3 @@\n h\n i\n+j\n");
    let original = b"x\ny\na\nb\nc\nd\ne\nf\ng\nh\ni\n";
    let applied = apply(original, &patch);
    assert!(applied.clean(), "{:?}", applied.hunks);
    assert_eq!(applied.theirs, b"x\ny\na\nb\nC\nd\ne\nf\ng\nh\ni\nj\n");
    assert_eq!(applied.hunks[0].offset, 2);
    assert_eq!(applied.hunks[0].line, 4);
    assert_eq!(applied.hunks[1].offset, 2, "the second hunk is searched from the first one's offset");
}

#[test]
fn trailing_spaces_and_crlf_do_not_stop_a_hunk() {
    let patch = file("--- a/f\n+++ b/f\n@@ -1,3 +1,3 @@\n a\n-b\n+B\n c\n");
    let applied = apply(b"a  \r\nb\r\nc\r\n", &patch);
    assert!(applied.clean());
    assert!(!applied.hunks[0].exact, "git apply would not take it");
    assert_eq!(applied.theirs, b"a  \r\nB\r\nc\r\n", "the file's own lines and line breaks are kept");
    let crlf = file("--- a/f\n+++ b/f\n@@ -1,3 +1,3 @@\n a\r\n-b\r\n+B\r\n c\r\n");
    assert_eq!(apply(b"a\nb\nc\n", &crlf).theirs, b"a\nB\nc\n");
    let exact = apply(b"a\r\nb\r\nc\r\n", &crlf);
    assert!(exact.hunks[0].exact);
    assert_eq!(exact.theirs, b"a\r\nB\r\nc\r\n");
}

#[test]
fn hunks_without_context_at_an_end_apply_there_first_as_git_applies_them() {
    let start = file("--- a/f\n+++ b/f\n@@ -1,2 +1,2 @@\n-a\n+A\n b\n");
    let applied = apply(b"a\nb\nx\na\nb\n", &start);
    assert!(applied.hunks[0].exact);
    assert_eq!(applied.theirs, b"A\nb\nx\na\nb\n");
    let moved = apply(b"zero\na\nb\n", &start);
    assert!(moved.clean());
    assert!(!moved.hunks[0].exact, "git apply refuses it: the hunk must match at the start");
    assert_eq!(moved.hunks[0].fuzz, 0);
    let end = file("--- a/f\n+++ b/f\n@@ -2,2 +2,2 @@\n a\n-b\n+B\n");
    let applied = apply(b"a\nb\nx\na\nb\n", &end);
    assert!(applied.hunks[0].exact);
    assert_eq!(applied.theirs, b"a\nb\nx\na\nB\n", "at the end, though the start is nearer");
    assert!(!apply(b"a\nb\nafter\n", &end).hunks[0].exact);
}

#[test]
fn a_line_without_a_line_break_is_only_ever_put_at_the_end() {
    let patch = file("--- a/f\n+++ b/f\n@@ -2,2 +2,2 @@\n a\n-b\n+b\n\\ No newline at end of file\n");
    assert_eq!(apply(b"x\na\nb\n", &patch).theirs, b"x\na\nb");
    let elsewhere = apply(b"x\na\nb\nmore\n", &patch);
    assert!(!elsewhere.clean(), "never `bmore`");
}

#[test]
fn a_patch_past_the_most_files_or_hunks_is_refused() {
    let hunk = "@@ -1 +1 @@\n-a\n+b\n";
    let hunks = format!("--- a/f\n+++ b/f\n{}", hunk.repeat(MAX_ITEMS + 1));
    assert_eq!(parse(hunks.as_bytes()), Err(PatchError::TooLarge));
    let files = format!("--- a/f\n+++ b/f\n{hunk}").repeat(MAX_ITEMS + 1);
    assert_eq!(parse(files.as_bytes()), Err(PatchError::TooLarge));
}

#[test]
fn a_hunk_never_applies_over_the_one_before_it() {
    let patch = file("--- a/f\n+++ b/f\n@@ -1 +1 @@\n-x\n+y\n@@ -2 +2 @@\n-x\n+w\n");
    let applied = apply(b"x\nz\n", &patch);
    assert_eq!(applied.hunks.iter().map(|hunk| hunk.applied).collect::<Vec<_>>(), [true, false]);
}

fn many_failing_hunks() -> (Vec<u8>, String) {
    let text: Vec<u8> = (0..200_000).flat_map(|n| format!("line {n}\n").into_bytes()).collect();
    let mut patch = String::from("--- a/f\n+++ b/f\n");
    for n in 0..3000 {
        patch.push_str(&format!("@@ -{0},2 +{0},2 @@\n line {1}\n-missing {n}\n+x\n", n * 10 + 1, n * 10));
    }
    (text, patch)
}

#[test]
fn many_failing_hunks_on_a_long_file_all_fail() {
    let (text, patch) = many_failing_hunks();
    let applied = apply(&text, &file(&patch));
    assert!(applied.hunks.iter().all(|hunk| !hunk.applied));
    assert_eq!(parse(&vec![b' '; MAX_BYTES + 1]), Err(PatchError::TooLarge));
}

#[test]
#[ignore = "a timing check; run by hand"]
fn many_failing_hunks_on_a_long_file_stay_cheap() {
    let (text, patch) = many_failing_hunks();
    let started = std::time::Instant::now();
    assert!(apply(&text, &file(&patch)).hunks.iter().all(|hunk| !hunk.applied));
    assert!(started.elapsed() < std::time::Duration::from_secs(5), "{:?}", started.elapsed());
}

#[test]
fn modes_that_are_not_plain_files_and_executable_bits_are_told() {
    let link = file(
        "diff --git a/l b/l\nnew file mode 120000\n--- /dev/null\n+++ b/l\n@@ -0,0 +1 @@\n+/etc/passwd\n\\ No newline at end of file\n",
    );
    assert!(link.is_special());
    let module = file(
        "diff --git a/m b/m\nindex 1111111..2222222 160000\n--- a/m\n+++ b/m\n@@ -1 +1 @@\n-Subproject commit 1\n+Subproject commit 2\n",
    );
    assert!(module.is_special());
    let tool = file("diff --git a/t b/t\nnew file mode 100755\n--- /dev/null\n+++ b/t\n@@ -0,0 +1 @@\n+x\n");
    assert!(!tool.is_special());
    assert_eq!(tool.executable_change(), Some(true));
    let chmod = file("diff --git a/t b/t\nold mode 100755\nnew mode 100644\n");
    assert_eq!(chmod.executable_change(), Some(false));
    let same = file("diff --git a/t b/t\nindex 1111111..2222222 100755\n--- a/t\n+++ b/t\n@@ -1 +1 @@\n-a\n+b\n");
    assert_eq!(same.executable_change(), None);
}

#[test]
fn context_at_the_ends_is_left_out_when_it_changed() {
    let patch = file("--- a/f\n+++ b/f\n@@ -1,5 +1,5 @@\n a\n b\n-c\n+C\n d\n e\n");
    let applied = apply(b"A\nb\nc\nd\nE\n", &patch);
    assert!(applied.clean(), "{:?}", applied.hunks);
    assert_eq!(applied.hunks[0].fuzz, 1);
    assert_eq!(applied.theirs, b"A\nb\nC\nd\nE\n");
}

#[test]
fn a_failed_hunk_is_reported_and_merges_with_markers() {
    let patch = file(
        "--- a/f\n+++ b/f\n@@ -1,3 +1,3 @@\n one\n-two\n+TWO\n three\n@@ -5,3 +5,3 @@\n five\n-six\n+SIX\n seven\n",
    );
    let original = b"one\ntwo\nthree\nfour\nfive\nmine\nseven\n";
    let applied = apply(original, &patch);
    assert!(!applied.clean());
    assert!(applied.hunks[0].applied);
    assert!(!applied.hunks[1].applied);
    assert_eq!(applied.hunks[1].line, 5);
    assert_eq!(applied.base, b"one\ntwo\nthree\nfour\nfive\nsix\nseven\n");
    assert_eq!(applied.theirs, b"one\nTWO\nthree\nfour\nfive\nSIX\nseven\n");
    let (merged, conflicts) =
        merge_with_markers(&applied.base, original, &applied.theirs, ("Current", "Patch")).unwrap();
    assert_eq!(conflicts, 1);
    assert_eq!(
        String::from_utf8(merged).unwrap(),
        "one\nTWO\nthree\nfour\nfive\n<<<<<<< Current\nmine\n=======\nSIX\n>>>>>>> Patch\nseven\n"
    );
}

#[test]
fn a_new_file_and_an_appended_line_apply_to_empty_and_short_files() {
    let new = file("--- /dev/null\n+++ b/n\n@@ -0,0 +1,2 @@\n+a\n+b\n");
    assert_eq!(apply(b"", &new).theirs, b"a\nb\n");
    let append = file("--- a/f\n+++ b/f\n@@ -1 +1,2 @@\n-x\n\\ No newline at end of file\n+x\n+y\n");
    let applied = apply(b"x", &append);
    assert!(applied.clean());
    assert_eq!(applied.theirs, b"x\ny\n");
    assert_eq!(apply(b"x\n", &append).theirs, b"x\ny\n", "a line break the patch lacks is tolerated");
}

#[test]
fn paths_never_leave_the_folder() {
    for bad in [
        "",
        "/etc/passwd",
        "../x",
        "a/../../x",
        "a/.git/hooks/pre-commit",
        ".GIT/config",
        "C:/x",
        "a\\..\\x",
        "a\0b",
        "./.",
        ".Git/hooks/x",
        ".git\u{200C}/config",
        ".g\u{200D}it/config",
        "\u{FEFF}.git/config",
        ".git./config",
        ".git /config",
        "GIT~1/config",
    ] {
        assert!(safe_path(bad).is_err(), "{bad:?}");
    }
    assert_eq!(safe_path("./a//b/./c.txt").unwrap(), "a/b/c.txt");
    assert_eq!(safe_path("a/.gitignore").unwrap(), "a/.gitignore");
    assert_eq!(safe_path(".gitmodules").unwrap(), ".gitmodules");
    assert_eq!(safe_path("x.git/y").unwrap(), "x.git/y");
    let escaping = parse(b"--- a/../../etc/passwd\n+++ b/../../etc/passwd\n@@ -1 +1 @@\n-a\n+b\n").unwrap();
    assert!(matches!(escaping.strip(1), Err(PatchError::UnsafePath(_))));
    let absolute = parse(b"--- /etc/passwd\n+++ /etc/passwd\n@@ -1 +1 @@\n-a\n+b\n").unwrap();
    assert!(absolute.strip(0).is_err());
    assert!(matches!(absolute.strip(9), Err(PatchError::Strip(_))));
    let into_git =
        parse(b"diff --git a/.git/config b/.git/config\n--- a/.git/config\n+++ b/.git/config\n@@ -1 +1 @@\n-a\n+b\n")
            .unwrap();
    assert!(into_git.strip(1).is_err());
}

#[test]
fn a_patch_git_made_keeps_every_path_the_repository_has() {
    let made = parse(b"diff --git \"a/tab\\tname\" \"b/tab\\tname\"\nnew file mode 100644\n--- /dev/null\n+++ \"b/tab\\tname\"\n@@ -0,0 +1 @@\n+x\n").unwrap();
    assert!(made.strip(1).is_err(), "never applied from someone else's patch");
    let own = made.without_git_prefixes();
    assert_eq!(own.files[0].path(), "tab\tname");
    assert!(String::from_utf8(own.to_bytes()).unwrap().contains("diff --git \"a/tab\\tname\" \"b/tab\\tname\"\n"));
}

#[test]
fn paths_move_under_and_out_of_folders() {
    let patch = parse(b"--- a/src/x.go\n+++ b/src/x.go\n@@ -1 +1 @@\n-a\n+b\n").unwrap().strip(1).unwrap();
    assert_eq!(patch.under("app/").unwrap().files[0].path(), "app/src/x.go");
    assert_eq!(patch.relative_to("src").unwrap().files[0].path(), "x.go");
    assert!(matches!(patch.relative_to("lib"), Err(PatchError::OutsideBase(_))));
    assert!(patch.under("../up").is_err());
}

#[test]
fn malformed_hunks_are_errors() {
    assert_eq!(parse(b"hello\n"), Err(PatchError::Empty));
    assert_eq!(parse(b""), Err(PatchError::Empty));
    assert_eq!(parse(b"--- a/f\n+++ b/f\n@@ -1,3 +1,3 @@\n a\n-b\n"), Err(PatchError::Truncated(3)));
    assert_eq!(parse(b"--- a/f\n+++ b/f\n@@ -x +1 @@\n a\n"), Err(PatchError::BadHunk(3)));
    assert_eq!(parse(b"--- a/f\n+++ b/f\n@@ -1 +1 @@\n+a\n+b\n"), Err(PatchError::Truncated(3)));
    assert_eq!(parse(b"--- a/f\n+++ b/f\n@@ -1,99999999999999 +1 @@\n a\n"), Err(PatchError::BadHunk(3)));
}

#[test]
fn a_patch_copied_without_its_last_line_break_still_applies() {
    let patch = file("--- a/f\n+++ b/f\n@@ -1,2 +1,2 @@\n-a\n+A\n b");
    assert!(apply(b"a\nb\n", &patch).clean());
}

#[test]
fn encodings_convert_both_ways() {
    let utf8 = "--- a/f\n+++ b/f\n@@ -1 +1 @@\n-привет\n+мир\n".as_bytes();
    let cp1251 = encode(utf8, "windows-1251").unwrap();
    assert_ne!(cp1251, utf8);
    assert!(std::str::from_utf8(&cp1251).is_err());
    assert_eq!(decode(&cp1251, "cp1251").unwrap(), utf8);
    assert_eq!(decode(b"\xEF\xBB\xBFabc", "utf-8").unwrap(), b"abc");
    let latin = b"caf\xe9\n";
    assert_eq!(encode(latin, "windows-1251").unwrap(), latin, "bytes that are not UTF-8 stay as they are");
    assert_eq!(encoding_name("CP1251"), Some("windows-1251"));
    assert_eq!(encoding_name("klingon"), None);
    assert!(encode(utf8, "klingon").is_none());
}

#[test]
fn mail_dates_are_written_as_git_writes_them() {
    assert_eq!(mail_date(1_792_620_301, 8 * 3600), "Thu, 22 Oct 2026 06:05:01 +0800");
    assert_eq!(mail_date(0, 0), "Thu, 1 Jan 1970 00:00:00 +0000");
    assert_eq!(mail_date(951_782_400, -(3 * 3600 + 1800)), "Mon, 28 Feb 2000 20:30:00 -0330");
}

#[test]
fn encoded_header_words_are_decoded() {
    assert_eq!(decode_words("=?UTF-8?q?Ad=C3=A1?= <a@b>"), "Adá <a@b>");
    assert_eq!(decode_words("=?utf-8?b?0J/RgNC40LLQtdGC?="), "Привет");
    assert_eq!(decode_words("=?UTF-8?q?a?= =?UTF-8?q?b?="), "ab");
    assert_eq!(decode_words("plain =? not a word"), "plain =? not a word");
    assert_eq!(decode_words("=?UTF-8?q?=ZZ?="), "=ZZ");
}

fn arbitrary_line() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(prop_oneof![Just(b'a'), Just(b' '), Just(b'\t'), Just(b'x'), Just(0xd0u8), Just(b'\r')], 0..6)
        .prop_map(|mut text| {
            text.push(b'\n');
            text
        })
}

fn arbitrary_hunk() -> impl Strategy<Value = Hunk> {
    prop::collection::vec((0..3u8, arbitrary_line()), 1..8).prop_map(|lines| {
        let lines: Vec<Line> = lines
            .into_iter()
            .map(|(kind, text)| Line {
                kind: match kind {
                    0 => LineKind::Context,
                    1 => LineKind::Removed,
                    _ => LineKind::Added,
                },
                text,
            })
            .collect();
        let old_lines = lines.iter().filter(|line| line.kind != LineKind::Added).count();
        let new_lines = lines.iter().filter(|line| line.kind != LineKind::Removed).count();
        Hunk {
            old_start: 1,
            old_lines,
            new_start: 1,
            new_lines,
            section: Vec::new(),
            lines: order_removed_first(lines),
        }
    })
}

fn arbitrary_patch() -> impl Strategy<Value = Patch> {
    let name = "[a-z]{1,6}(/[a-z]{1,6}){0,2}(\\.go)?";
    prop::collection::vec((name, prop::collection::vec(arbitrary_hunk(), 1..4)), 1..4).prop_map(|files| Patch {
        mail: None,
        files: files
            .into_iter()
            .map(|(path, hunks)| FilePatch {
                old_path: Some(path.clone()),
                new_path: Some(path),
                hunks,
                ..FilePatch::default()
            })
            .collect(),
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn any_bytes_are_read_without_panicking(bytes in prop::collection::vec(any::<u8>(), 0..400)) {
        if let Ok(patch) = parse(&bytes) {
            let _ = patch.strip(1).map(|patch| patch.to_bytes());
            let _ = patch.reversed().to_bytes();
            for file in &patch.files {
                let _ = apply(&bytes, file);
            }
        }
    }

    #[test]
    fn broken_patches_are_read_without_panicking(
        cut in 0usize..2000,
        flips in prop::collection::vec((0usize..2000, any::<u8>()), 0..6),
    ) {
        let mut bytes = FORMAT_PATCH.as_bytes().to_vec();
        for (at, byte) in flips {
            let at = at % bytes.len();
            bytes[at] = byte;
        }
        bytes.truncate(cut % (bytes.len() + 1));
        if let Ok(patch) = parse(&bytes) {
            let _ = patch.strip(1);
            let _ = patch.under("x");
            let _ = patch.relative_to("x");
            let _ = patch.reversed().to_bytes();
            for file in &patch.files {
                let _ = apply(b"one\ntwo\nthree\n", file);
            }
        }
    }

    #[test]
    fn written_patches_read_back_the_same(patch in arbitrary_patch()) {
        let written = patch.to_bytes();
        let again = parse(&written).unwrap().strip(1).unwrap();
        prop_assert_eq!(again, patch);
    }

    #[test]
    fn a_patch_applies_to_its_own_old_text_and_back(patch in arbitrary_patch()) {
        for file in &patch.files {
            let hunk = &file.hunks[0];
            let single = FilePatch { hunks: vec![hunk.clone()], ..file.clone() };
            let old: Vec<u8> = hunk.preimage().concat();
            let new: Vec<u8> = hunk.postimage().concat();
            let applied = apply(&old, &single);
            prop_assert!(applied.clean());
            prop_assert_eq!(&applied.theirs, &new);
            let back = apply(&new, &Patch { mail: None, files: vec![single] }.reversed().files[0]);
            prop_assert!(back.clean());
            prop_assert_eq!(back.theirs, old);
        }
    }

    #[test]
    fn applying_to_any_text_never_panics(
        patch in arbitrary_patch(),
        text in prop::collection::vec(arbitrary_line(), 0..20),
    ) {
        let text = text.concat();
        for file in &patch.files {
            let applied = apply(&text, file);
            if !applied.clean() {
                let _ = merge_with_markers(&applied.base, &text, &applied.theirs, ("a", "b"));
            }
        }
    }

    #[test]
    fn safe_paths_stay_inside(path in "[a-z./\\\\:]{0,12}") {
        if let Ok(safe) = safe_path(&path) {
            prop_assert!(!safe.starts_with('/'));
            prop_assert!(!safe.split('/').any(|part| part == ".." || part.is_empty() || part == "."));
        }
    }
}
