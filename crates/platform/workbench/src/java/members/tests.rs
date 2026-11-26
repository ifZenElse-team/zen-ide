use super::*;

const DOG: &str = "package com.acme;

import java.util.List;
import java.util.Map;

/** A dog. */
@Deprecated
public final class Dog<T> extends Animal implements Pet, Comparable<Dog<T>> {
    public static final int LEGS = 4, EARS = 2;
    private String owner = \"me\";
    List<String> tricks = new java.util.ArrayList<>() {{ add(\"sit\"); }};

    /** Makes one. */
    public Dog(String owner) {
        this.owner = owner;
    }

    @Override
    public String bark(final int times, Map<String, List<Integer>> extra) {
        return name + owner + times;
    }

    abstract void sleep();

    static {
        System.out.println(\"loaded\");
    }

    class Inner {
        int depth;
    }
}
";

#[test]
fn a_class_lists_its_header_and_members_with_their_kinds_names_and_types() {
    let at = DOG.find("bark").unwrap();
    let dog = class_at(DOG, at).unwrap();
    assert_eq!((dog.keyword.as_str(), dog.name.as_str()), ("class", "Dog"));
    assert!(dog.has("final") && dog.has("public"));
    assert_eq!(dog.extends.iter().map(|name| name.name.as_str()).collect::<Vec<_>>(), ["Animal"]);
    assert_eq!(dog.implements.iter().map(|name| name.name.as_str()).collect::<Vec<_>>(), ["Pet", "Comparable"]);
    assert_eq!(&DOG[dog.implements[1].start..dog.implements[1].end], "Comparable<Dog<T>>");
    assert_eq!(&DOG[dog.after_name - 3..dog.after_name], "<T>");
    let kinds: Vec<(MemberKind, &str)> = dog.members.iter().map(|member| (member.kind, member.name.as_str())).collect();
    assert_eq!(
        kinds,
        [
            (MemberKind::Field, "LEGS"),
            (MemberKind::Field, "owner"),
            (MemberKind::Field, "tricks"),
            (MemberKind::Constructor, "Dog"),
            (MemberKind::Method, "bark"),
            (MemberKind::Method, "sleep"),
            (MemberKind::Initializer, ""),
            (MemberKind::Type, "Inner"),
        ]
    );
    assert_eq!(dog.members[0].names, ["LEGS", "EARS"]);
    assert_eq!(dog.members[2].kind_text, "List<String>");
    let bark = &dog.members[4];
    assert_eq!(bark.label(), "bark(int, Map<String, List<Integer>>): String");
    assert_eq!(bark.parameters[1].name, "extra");
    assert!(bark.has("public") && bark.body.is_some());
    assert!(dog.members[5].body.is_none() && dog.members[5].has("abstract"));
    assert_eq!(&DOG[bark.first..bark.first + 9], "@Override");
    let inner = class_at(DOG, DOG.find("depth").unwrap()).unwrap();
    assert_eq!(inner.name, "Inner");
}

#[test]
fn a_member_block_takes_its_doc_comment_and_its_line_and_moves_to_another_indent() {
    let dog = class_named(DOG, "Dog").unwrap();
    let constructor = &dog.members[3];
    let (from, to) = block(DOG, constructor);
    assert!(DOG[from..to].starts_with("    /** Makes one. */\n    public Dog("), "{}", &DOG[from..to]);
    assert!(DOG[from..to].ends_with("    }\n"));
    assert_eq!(reindent("    int a;\n\n    int b;\n", "    ", "        "), "        int a;\n\n        int b;\n");
}

#[test]
fn members_go_before_the_closing_brace_or_into_an_empty_body() {
    let empty = "class A {\n}\n";
    let class = class_named(empty, "A").unwrap();
    let (from, to, text) = insertion(empty, &class, "    int a;\n");
    let mut out = empty.to_owned();
    out.replace_range(from..to, &text);
    assert_eq!(out, "class A {\n    int a;\n}\n");
    let full = "class A {\n    int b;\n}\n";
    let class = class_named(full, "A").unwrap();
    let (from, to, text) = insertion(full, &class, "    int a;\n");
    let mut out = full.to_owned();
    out.replace_range(from..to, &text);
    assert_eq!(out, "class A {\n    int b;\n\n    int a;\n}\n");
}

#[test]
fn imports_a_moved_member_uses_are_the_ones_the_destination_lacks() {
    let destination = "package com.acme;\n\nimport java.util.List;\n\nclass Animal {\n}\n";
    let needed = needed_imports(DOG, destination, &["Map<String, List<Integer>> extra;"]);
    assert_eq!(needed, ["import java.util.Map;"]);
    let end = destination.find("List;").unwrap() + 5;
    assert_eq!(import_insertion(destination, &needed), Some((end, "\nimport java.util.Map;".to_owned())));
    assert_eq!(package(DOG), "com.acme");
}

#[test]
fn a_calls_arguments_are_split_at_its_own_commas() {
    let call = "total(f(1, 2), \"a,b\", new int[] {1, 2})";
    let (close, arguments) = split_arguments(call, 5).unwrap();
    assert_eq!(close, call.len() - 1);
    let texts: Vec<&str> =
        arguments.iter().map(|range| &call[trimmed(call, *range).0..trimmed(call, *range).1]).collect();
    assert_eq!(texts, ["f(1, 2)", "\"a,b\"", "new int[] {1, 2}"]);
    assert_eq!(split_arguments("f()", 1).unwrap().1, Vec::<(usize, usize)>::new());
}
