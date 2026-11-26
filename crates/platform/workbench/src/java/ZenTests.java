import java.io.PrintWriter;
import java.io.StringWriter;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.HashSet;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.Set;

import org.junit.platform.engine.DiscoverySelector;
import org.junit.platform.engine.TestExecutionResult;
import org.junit.platform.engine.TestSource;
import org.junit.platform.engine.discovery.DiscoverySelectors;
import org.junit.platform.engine.support.descriptor.ClassSource;
import org.junit.platform.engine.support.descriptor.MethodSource;
import org.junit.platform.launcher.Launcher;
import org.junit.platform.launcher.LauncherDiscoveryRequest;
import org.junit.platform.launcher.TestExecutionListener;
import org.junit.platform.launcher.TestIdentifier;
import org.junit.platform.launcher.core.LauncherDiscoveryRequestBuilder;
import org.junit.platform.launcher.core.LauncherFactory;

public class ZenTests {
    public static void main(String[] args) throws Exception {
        List<DiscoverySelector> selectors = new ArrayList<>();
        for (String arg : args) {
            int colon = arg.indexOf(':');
            String kind = arg.substring(0, colon);
            String value = arg.substring(colon + 1);
            if (kind.equals("roots")) {
                Set<Path> roots = new HashSet<>();
                for (String line : Files.readAllLines(Path.of(value))) {
                    if (!line.isBlank()) roots.add(Path.of(line.trim()));
                }
                selectors.addAll(DiscoverySelectors.selectClasspathRoots(roots));
            } else if (kind.equals("package")) {
                selectors.add(DiscoverySelectors.selectPackage(value));
            } else if (kind.equals("class")) {
                selectors.add(DiscoverySelectors.selectClass(value));
            } else if (kind.equals("method")) {
                selectors.addAll(methods(value));
            } else {
                throw new IllegalArgumentException(arg);
            }
        }
        LauncherDiscoveryRequest request = LauncherDiscoveryRequestBuilder.request().selectors(selectors).build();
        Launcher launcher = LauncherFactory.create();
        Reporter reporter = new Reporter();
        launcher.execute(request, reporter);
        System.out.flush();
        System.exit(reporter.failed ? 1 : 0);
    }

    static List<DiscoverySelector> methods(String value) throws ClassNotFoundException {
        List<DiscoverySelector> found = new ArrayList<>();
        int hash = value.indexOf('#');
        String name = value.substring(hash + 1);
        if (name.contains("(")) {
            found.add(DiscoverySelectors.selectMethod(value));
            return found;
        }
        Class<?> type = Class.forName(value.substring(0, hash), false, Thread.currentThread().getContextClassLoader());
        for (Class<?> at = type; at != null && at != Object.class; at = at.getSuperclass()) {
            for (java.lang.reflect.Method method : at.getDeclaredMethods()) {
                if (method.getName().equals(name)) found.add(DiscoverySelectors.selectMethod(type, method));
            }
        }
        if (found.isEmpty()) found.add(DiscoverySelectors.selectMethod(value));
        return found;
    }

    static final class Reporter implements TestExecutionListener {
        final Map<String, Long> started = new HashMap<>();
        boolean failed;

        static String className(TestIdentifier id) {
            Optional<TestSource> source = id.getSource();
            if (source.isPresent() && source.get() instanceof MethodSource) {
                return ((MethodSource) source.get()).getClassName();
            }
            if (source.isPresent() && source.get() instanceof ClassSource) {
                return ((ClassSource) source.get()).getClassName();
            }
            return null;
        }

        static boolean isClass(TestIdentifier id) {
            return id.getSource().isPresent() && id.getSource().get() instanceof ClassSource;
        }

        static String quote(String text) {
            StringBuilder out = new StringBuilder("\"");
            for (char c : text.toCharArray()) {
                if (c == '"' || c == '\\') out.append('\\').append(c);
                else if (c < 0x20) out.append(String.format("\\u%04x", (int) c));
                else out.append(c);
            }
            return out.append('"').toString();
        }

        static void report(String event, String suite, String test, String result, Long ms, String stream, String text) {
            StringBuilder line = new StringBuilder("##zen-test {\"event\":").append(quote(event));
            line.append(",\"suite\":").append(quote(suite)).append(",\"test\":").append(quote(test));
            if (result != null) line.append(",\"result\":").append(quote(result));
            if (ms != null) line.append(",\"ms\":").append(ms);
            if (stream != null) line.append(",\"stream\":").append(quote(stream));
            if (text != null) line.append(",\"text\":").append(quote(text));
            System.out.println(line.append('}'));
        }

        String name(TestIdentifier id) {
            String suite = className(id);
            return isClass(id) ? suite : id.getLegacyReportingName();
        }

        @Override
        public void executionStarted(TestIdentifier id) {
            String suite = className(id);
            if (suite == null) return;
            started.put(id.getUniqueId(), System.currentTimeMillis());
            report(id.isContainer() ? "suiteStarted" : "testStarted", suite, name(id), null, null, null, null);
        }

        @Override
        public void executionSkipped(TestIdentifier id, String reason) {
            String suite = className(id);
            if (suite == null) return;
            report(id.isContainer() ? "suiteStarted" : "testStarted", suite, name(id), null, null, null, null);
            String event = id.isContainer() ? "suiteFinished" : "testFinished";
            report(event, suite, name(id), "SKIPPED", 0L, null, null);
        }

        @Override
        public void executionFinished(TestIdentifier id, TestExecutionResult result) {
            Optional<Throwable> thrown = result.getThrowable();
            String suite = className(id);
            if (suite == null) {
                if (thrown.isPresent()) {
                    failed = true;
                    thrown.get().printStackTrace();
                }
                return;
            }
            boolean failure = result.getStatus() == TestExecutionResult.Status.FAILED;
            String status = failure
                ? "FAILURE"
                : result.getStatus() == TestExecutionResult.Status.ABORTED ? "SKIPPED" : "SUCCESS";
            failed |= failure;
            if (thrown.isPresent() && failure) {
                StringWriter writer = new StringWriter();
                thrown.get().printStackTrace(new PrintWriter(writer));
                for (String line : writer.toString().split("\\R")) {
                    report("output", suite, name(id), null, null, "StdErr", line);
                }
            }
            Long start = started.remove(id.getUniqueId());
            long ms = start == null ? 0 : System.currentTimeMillis() - start;
            report(id.isContainer() ? "suiteFinished" : "testFinished", suite, name(id), status, ms, null, null);
        }
    }
}
