use super::*;

fn kotlin(java: &str) -> String {
    let conversion = java_to_kotlin(java);
    assert!(conversion.problems.is_empty(), "{:?}\n{}", conversion.problems, conversion.text);
    conversion.text
}

fn same(java: &str, expected: &str) {
    let got = kotlin(java);
    assert_eq!(got.trim_end(), expected.trim_end(), "\n--- got ---\n{got}");
}

#[test]
fn a_class_with_fields_a_constructor_and_methods() {
    same(
        r#"package com.example;

import java.util.List;
import java.util.ArrayList;

/** A greeter. */
public class Greeter {
    private final String name;
    private int count = 0;

    public Greeter(String name) {
        this.name = name;
    }

    // Says hello.
    public String greet(String other) {
        count++;
        return "Hello, " + other + " from " + name;
    }

    public int getCount() {
        return count;
    }
}
"#,
        r#"package com.example

import java.util.ArrayList

/** A greeter. */
class Greeter(private val name: String) {
    private var count: Int = 0

    // Says hello.
    fun greet(other: String): String {
        count++
        return "Hello, " + other + " from " + name
    }

    fun getCount(): Int {
        return count
    }
}
"#,
    );
}

#[test]
fn statics_go_to_a_companion_object_and_main_stays_runnable() {
    same(
        r#"public class App {
    public static final int LIMIT = 10;
    private static final String NAME = "app";

    public static void main(String[] args) {
        for (int i = 0; i < LIMIT; i++) {
            System.out.println(NAME + i);
        }
    }
}
"#,
        r#"class App {
    companion object {
        const val LIMIT: Int = 10
        private const val NAME: String = "app"

        @JvmStatic
        fun main(args: Array<String>) {
            for (i in 0 until LIMIT) {
                println(NAME + i)
            }
        }
    }
}
"#,
    );
}

#[test]
fn locals_are_val_unless_reassigned_and_loops_become_kotlin_loops() {
    same(
        r#"class Sum {
    int total(int[] values, List<String> names) {
        int sum = 0;
        final int start = 2;
        for (int value : values) {
            sum += value;
        }
        for (String name : names) {
            if (name.length() > start) {
                sum++;
            } else if (name.isEmpty()) {
                continue;
            } else {
                sum--;
            }
        }
        while (sum > 100) {
            sum /= 2;
        }
        return sum;
    }
}
"#,
        r#"internal class Sum {
    internal fun total(values: IntArray, names: MutableList<String>): Int {
        var sum = 0
        val start = 2
        for (value in values) {
            sum += value
        }
        for (name in names) {
            if (name.length > start) {
                sum++
            } else if (name.isEmpty()) {
                continue
            } else {
                sum--
            }
        }
        while (sum > 100) {
            sum /= 2
        }
        return sum
    }
}
"#,
    );
}

#[test]
fn expressions_take_their_kotlin_forms() {
    same(
        r#"public class E {
    public String f(Object o, int a, int b, long[] xs) {
        String s = a > b ? "big" : "small";
        int bits = (a & 0xff) | (b << 2) ^ ~a;
        long first = (long) a;
        int n = xs.length;
        boolean text = o instanceof String;
        String cast = (String) o;
        Class<?> c = String.class;
        char ch = s.charAt(0);
        return String.valueOf(n) + c.getName() + cast + ch + bits + first + text;
    }
}
"#,
        r#"class E {
    fun f(o: Any, a: Int, b: Int, xs: LongArray): String {
        val s: String = if (a > b) "big" else "small"
        val bits: Int = (a and 0xff) or ((b shl 2) xor a.inv())
        val first: Long = a.toLong()
        val n: Int = xs.size
        val text: Boolean = o is String
        val cast: String = o as String
        val c: Class<*> = String::class.java
        val ch: Char = s[0]
        return n.toString() + c.getName() + cast + ch + bits + first + text
    }
}
"#,
    );
}

#[test]
fn lambdas_anonymous_classes_and_try_with_resources() {
    same(
        r#"public class L {
    public void run(List<Integer> items) throws IOException {
        items.forEach(item -> System.out.println(item));
        Runnable task = new Runnable() {
            @Override
            public void run() {
                System.out.println("ran");
            }
        };
        try (BufferedReader reader = open()) {
            reader.readLine();
        } catch (IOException | RuntimeException e) {
            throw new IllegalStateException(e);
        } finally {
            task.run();
        }
    }
}
"#,
        r#"class L {
    fun run(items: MutableList<Int>) {
        items.forEach({ item -> println(item) })
        val task: Runnable = object : Runnable {
            override fun run() {
                println("ran")
            }
        }
        try {
            open().use { reader ->
                reader.readLine()
            }
        } catch (e: IOException) {
            throw IllegalStateException(e)
        } catch (e: RuntimeException) {
            throw IllegalStateException(e)
        } finally {
            task.run()
        }
    }
}
"#,
    );
}

#[test]
fn switch_becomes_when_and_enums_keep_their_constants() {
    same(
        r#"public enum Color {
    RED("r"), GREEN("g");

    private final String code;

    Color(String code) {
        this.code = code;
    }

    public static String describe(int n) {
        switch (n) {
            case 1:
            case 2:
                return "few";
            case 3:
                System.out.println("three");
                break;
            default:
                return "many";
        }
        return "three";
    }
}
"#,
        r#"enum class Color(private val code: String) {
    RED("r"),
    GREEN("g");

    companion object {
        fun describe(n: Int): String {
            when (n) {
                1, 2 -> return "few"
                3 -> println("three")
                else -> return "many"
            }
            return "three"
        }
    }
}
"#,
    );
}

#[test]
fn interfaces_generics_records_and_inheritance() {
    same(
        r#"public interface Shape<T extends Number> {
    double area();
    default String label() { return "shape"; }
}

public record Point(int x, int y) {}

public class Base {
    protected int size;
    public void grow() { size++; }
}

public class Child extends Base implements Comparable<Child> {
    @Override
    public void grow() { size += 2; }

    @Override
    public int compareTo(Child other) {
        return Integer.compare(size, other.size);
    }
}
"#,
        r#"interface Shape<T : Number> {
    fun area(): Double

    fun label(): String {
        return "shape"
    }
}

data class Point(val x: Int, val y: Int)

open class Base {
    protected var size: Int = 0

    open fun grow() {
        size++
    }
}

class Child : Base(), Comparable<Child> {
    override fun grow() {
        size += 2
    }

    override fun compareTo(other: Child): Int {
        return Integer.compare(size, other.size)
    }
}
"#,
    );
}

#[test]
fn literals_strings_and_names_are_valid_kotlin() {
    same(
        r#"class S {
    void f() {
        long big = 10l;
        int octal = 017;
        double d = 1d;
        float f = .5f;
        String money = "costs $5\t";
        String block = """
            Hello,
              "world"
            """;
        Object in = null;
    }
}
"#,
        r#"internal class S {
    internal fun f() {
        val big = 10L
        val octal = 15
        val d = 1.0
        val f = 0.5f
        val money = "costs \$5\t"
        val block = "Hello,\n  \"world\"\n"
        val `in`: Any? = null
    }
}
"#,
    );
}

#[test]
fn what_cannot_be_converted_is_kept_and_reported() {
    let conversion = java_to_kotlin(
        "class X {\n    void f(int n) {\n        switch (n) {\n            case 1:\n                g();\n            case 2:\n                h();\n                break;\n        }\n    }\n}\n",
    );
    assert_eq!(conversion.problems.len(), 1, "{:?}", conversion.problems);
    assert!(conversion.problems[0].message.contains("falls through"));
    let at = conversion.problems[0].start;
    assert!(conversion.text.contains("1 -> g()"), "{}", conversion.text);
    let java = "class X {\n    void f(int n) {\n        switch (n) {\n            case 1:\n";
    assert_eq!(&"class X {\n    void f(int n) {\n        switch (n) {\n            case 1:"[at..at + 4], "case");
    let _ = java;
    let broken = java_to_kotlin("class Y {\n    int x = ;\n    void ok() {}\n}\n");
    assert_eq!(broken.problems.len(), 1, "{:?}", broken.problems);
    assert!(broken.text.contains("/* Not converted:"), "{}", broken.text);
    assert!(broken.text.contains("int x = ;"), "{}", broken.text);
    assert!(broken.text.contains("fun ok()"), "the rest is converted: {}", broken.text);
}

#[test]
fn only_java_is_a_source() {
    assert!(convert("java", "class A {}").is_some());
    assert!(convert("scala", "class A {}").is_none());
    assert_eq!(SOURCES, "java");
}

#[test]
fn arguments_to_what_the_file_declares_are_widened_as_java_does() {
    same(
        r#"class W {
    W(double size, long count) {}
    void use(double value) {}
    void run(int i) {
        W w = new W(i, 3);
        use(i + 1);
        use(2.5);
    }
}
"#,
        r#"internal class W(size: Double, count: Long) {
    internal fun use(value: Double) {
    }

    internal fun run(i: Int) {
        val w = W(i.toDouble(), 3L)
        use((i + 1).toDouble())
        use(2.5)
    }
}
"#,
    );
}

#[test]
fn pasted_members_and_statements_are_converted_on_their_own() {
    let members = fragment("private int count;\n\npublic int next() {\n    return ++count;\n}\n");
    assert!(members.problems.is_empty(), "{:?}", members.problems);
    assert_eq!(members.text, "private var count: Int = 0\n\nfun next(): Int {\n    return ++count\n}\n");
    let statements =
        fragment("int total = 0;\nfor (int i = 0; i < 3; i++) {\n    total += i;\n}\nSystem.out.println(total);\n");
    assert!(statements.problems.is_empty(), "{:?}", statements.problems);
    assert_eq!(statements.text, "var total = 0\nfor (i in 0 until 3) {\n    total += i\n}\nprintln(total)\n");
    let file = fragment("package a;\n\npublic class A {}\n");
    assert_eq!(file.text, "package a\n\nclass A\n");
}
