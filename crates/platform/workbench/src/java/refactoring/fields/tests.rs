use serde_json::{Value, json};

use super::*;
use crate::java::refactoring::hierarchy::tests::{
    DOG, SHOP, at, dialog, forced, messages, range, rows, text, uri, zoo,
};

fn field(name: &str, getter: &str, setter: &str) -> Encapsulated {
    Encapsulated { name: name.to_owned(), getter: getter.to_owned(), setter: setter.to_owned() }
}

fn rewritten(text: &str, needle: &str, field: &Encapsulated) -> Result<String, String> {
    let from = text.find(needle).unwrap() + needle.find(&field.name).unwrap();
    let (start, end, new) = rewrite_access(text, from, from + field.name.len(), field)?;
    let mut out = text.to_owned();
    out.replace_range(start..end, &new);
    Ok(out)
}

#[test]
fn every_kind_of_access_becomes_an_accessor_call_or_a_reason_it_cannot() {
    let age = field("age", "getAge", "setAge");
    assert_eq!(rewritten("{ x = dog.age + 1; }", "dog.age", &age).unwrap(), "{ x = dog.getAge() + 1; }");
    assert_eq!(rewritten("{ dog.age = 3 * k; }", "dog.age", &age).unwrap(), "{ dog.setAge(3 * k); }");
    assert_eq!(rewritten("{ dog.age += a + b; }", "dog.age", &age).unwrap(), "{ dog.setAge(dog.getAge() + (a + b)); }");
    assert_eq!(rewritten("{ dog.age++; }", "dog.age", &age).unwrap(), "{ dog.setAge(dog.getAge() + 1); }");
    assert_eq!(rewritten("{ --dog.age; }", "dog.age", &age).unwrap(), "{ dog.setAge(dog.getAge() - 1); }");
    assert_eq!(rewritten("{ if (dog.age == 2) {} }", "dog.age", &age).unwrap(), "{ if (dog.getAge() == 2) {} }");
    let value = rewritten("{ x = (dog.age = 3); }", "dog.age", &age).unwrap_err();
    assert_eq!(value, t!("workbench-java-encapsulate-value", name = "age"));
    let twice = rewritten("{ find().age++; }", "find().age", &age).unwrap_err();
    assert_eq!(twice, t!("workbench-java-encapsulate-unsure", name = "age"), "the qualifier would run twice");
    let read_only = field("age", "getAge", "");
    let written = rewritten("{ dog.age = 1; }", "dog.age", &read_only).unwrap_err();
    assert_eq!(written, t!("workbench-java-encapsulate-no-setter", name = "age"));
}

#[test]
fn encapsulating_a_field_adds_its_accessors_hides_it_and_rewrites_uses_outside_the_class() {
    let mut fixture = zoo();
    let shop = uri(&fixture, "src/com/acme/Shop.java");
    fixture.server.on_with("textDocument/references", move |params| {
        assert_eq!(params["position"]["line"], 5, "asked at age");
        json!([
            { "uri": shop, "range": range(9, 12, 9, 15) },
            { "uri": shop, "range": range(10, 12, 10, 15) },
            { "uri": shop, "range": range(11, 31, 11, 34) }
        ])
    });
    let context = json!({ "path": "src/com/acme/Dog.java", "offset": at(DOG, "age;") });
    let form = dialog(&mut fixture, "refactor.dialog.form", ENCAPSULATE_FIELDS, context.clone(), json!({}));
    let fields = rows(&form, "fields");
    assert_eq!(
        fields[0],
        json!({ "selected": true, "field": "age: int", "name": "age", "getter": "getAge", "setter": "setAge" })
    );
    assert_eq!(fields[1]["selected"], false, "only the field at the caret");
    let plan = dialog(&mut fixture, "refactor.dialog.preview", ENCAPSULATE_FIELDS, context.clone(), json!({}));
    assert!(messages(&plan).is_empty(), "{plan}");
    forced(&mut fixture, ENCAPSULATE_FIELDS, context, json!({}));
    let shop = text(&mut fixture, "src/com/acme/Shop.java");
    assert!(shop.contains("        dog.setAge(3);\n        dog.setAge(dog.getAge() + 2);\n"), "{shop}");
    assert!(shop.contains("System.out.println(dog.getAge() + total("), "{shop}");
    let dog = text(&mut fixture, "src/com/acme/Dog.java");
    assert!(dog.contains("    private int age;\n"), "{dog}");
    assert!(
        dog.ends_with("    public int getAge() {\n        return age;\n    }\n\n    public void setAge(int age) {\n        this.age = age;\n    }\n}\n"),
        "{dog}"
    );
}

#[test]
fn introducing_a_parameter_object_creates_the_class_and_rewrites_the_method_and_its_calls() {
    let mut fixture = zoo();
    fixture.open("src/com/acme/Shop.java");
    let shop = uri(&fixture, "src/com/acme/Shop.java");
    let calls = json!([{ "uri": shop, "range": range(11, 37, 11, 53) }]);
    fixture.server.on("textDocument/references", calls);
    fixture.server.on("textDocument/implementation", json!([{ "uri": shop, "range": range(3, 22, 3, 27) }]));
    let context = json!({ "path": "src/com/acme/Shop.java", "offset": at(SHOP, "total(int") });
    let form = dialog(&mut fixture, "refactor.dialog.form", INTRODUCE_PARAMETER_OBJECT, context.clone(), json!({}));
    assert_eq!(form["fields"][0]["value"], "TotalParameters", "{form}");
    let mut parameters = rows(&form, "parameters");
    assert_eq!(parameters.len(), 3);
    parameters[2]["selected"] = json!(false);
    let values = json!({ "className": "Order", "parameterName": "order", "kind": "record", "parameters": parameters });
    let plan =
        dialog(&mut fixture, "refactor.dialog.preview", INTRODUCE_PARAMETER_OBJECT, context.clone(), values.clone());
    assert_eq!(plan["error"], Value::Null, "{plan}");
    assert_eq!(plan["operations"][0]["relative"], "src/com/acme/Order.java");
    dialog(&mut fixture, "refactor.dialog.apply", INTRODUCE_PARAMETER_OBJECT, context.clone(), values);
    let shop = text(&mut fixture, "src/com/acme/Shop.java");
    assert!(shop.contains("public static int total(Order order, String label) {\n        return order.price() * order.count() + label.length();"), "{shop}");
    assert!(shop.contains("total(new Order(1, 2), \"x\")"), "{shop}");
    assert_eq!(
        std::fs::read_to_string(fixture.path("src/com/acme/Order.java")).unwrap(),
        "package com.acme;\n\npublic record Order(int price, int count) {\n}\n"
    );
}

#[test]
fn a_parameter_object_is_refused_for_an_overridden_method_and_an_assigned_parameter() {
    let mut fixture = zoo();
    fixture.open("src/com/acme/Shop.java");
    let dog = uri(&fixture, "src/com/acme/Dog.java");
    fixture.server.on("textDocument/references", json!([]));
    fixture.server.on("textDocument/implementation", json!([{ "uri": dog, "range": range(9, 18, 9, 22) }]));
    let context = json!({ "path": "src/com/acme/Shop.java", "offset": at(SHOP, "total(int") });
    let plan = dialog(&mut fixture, "refactor.dialog.preview", INTRODUCE_PARAMETER_OBJECT, context, json!({}));
    let expected = t!("workbench-java-parameter-object-overridden", name = "total", file = "Dog.java");
    assert_eq!(plan["error"], json!(expected));

    let text = "class A {\n    int f(int a) {\n        a = 2;\n        return a;\n    }\n}\n";
    let class = members::class_at(text, 20).unwrap();
    let method = class.members[0].clone();
    let plan = ParameterObject {
        source: Path::new("/tmp/none/A.java"),
        text,
        class: &class,
        method: &method,
        class_name: "FParameters",
        parameter: "parameters",
        record: false,
        selected: &["a".to_owned()],
        calls: &[],
        texts: &[],
    };
    let refused = introduce_parameter_object(&plan, &mut Edits::default()).unwrap_err();
    assert_eq!(refused, t!("workbench-java-parameter-object-assigned", name = "a"));
}
