use crate::model::{self, File, Index};
use crate::yaml::{Value, documents};

fn repository(files: &[(&str, &str)], folders: &[&str]) -> (Vec<File>, Vec<String>) {
    let files: Vec<File> =
        files.iter().map(|(path, text)| File { path: (*path).to_owned(), text: (*text).to_owned() }).collect();
    let mut paths: Vec<String> = files.iter().map(|file| file.path.clone()).collect();
    paths.extend(folders.iter().map(|folder| (*folder).to_owned()));
    (files, paths)
}

fn text_at<'a>(files: &'a [File], path: &str, start: usize, end: usize) -> &'a str {
    &files.iter().find(|file| file.path == path).unwrap().text[start..end]
}

fn edges(index: &Index) -> Vec<(String, String, &'static str)> {
    index.edges.iter().map(|edge| (edge.from.clone(), edge.to.clone(), edge.kind)).collect()
}

#[test]
fn yaml_reads_maps_sequences_scalars_and_documents_with_their_spans() {
    let text = "---\n# a comment\napiVersion: v1   # trailing\nkind: \"Thing\"\nmetadata:\n  name: 'it''s'\n  labels: {app: web, tier: \"back\"}\nspec:\n  list:\n  - a\n  - name: b\n    value: 2\n  more: [x, y]\n  script: |\n    echo one\n    echo two\n  empty:\n---\nkind: Second\n";
    let docs = documents(text);
    assert_eq!(docs.len(), 2);
    let root = &docs[0].root;
    assert_eq!(root.get("apiVersion").and_then(|node| node.str()), Some("v1"));
    let kind = root.get("kind").unwrap();
    assert_eq!((kind.str(), &text[kind.start..kind.end]), (Some("Thing"), "\"Thing\""));
    assert_eq!(root.at(&["metadata", "name"]).and_then(|node| node.str()), Some("it's"));
    assert_eq!(root.at(&["metadata", "labels", "tier"]).and_then(|node| node.str()), Some("back"));
    let list = root.at(&["spec", "list"]).unwrap().items();
    assert_eq!(list[0].str(), Some("a"));
    let b = list[1].get("name").unwrap();
    assert_eq!(&text[b.start..b.end], "b");
    assert_eq!(list[1].get("value").and_then(|node| node.str()), Some("2"));
    let more: Vec<_> = root.at(&["spec", "more"]).unwrap().items().iter().map(|node| node.str().unwrap()).collect();
    assert_eq!(more, ["x", "y"]);
    assert_eq!(root.at(&["spec", "script"]).and_then(|node| node.str()), Some("echo one\necho two"));
    assert_eq!(root.at(&["spec", "empty"]).map(|node| &node.value), Some(&Value::Null));
    assert_eq!(docs[1].root.get("kind").and_then(|node| node.str()), Some("Second"));
}

#[test]
fn yaml_survives_any_input() {
    let pieces = ["-", " ", "a", ":", "\n", "#", "'", "\"", "[", "]", "{", "}", ",", "|", "---", "é", "\t", "  -"];
    for a in pieces {
        for b in pieces {
            for c in pieces {
                for text in [format!("{a}{b}{c}"), format!("{a}\n{b}\n  {c}")] {
                    for doc in documents(&text) {
                        assert!(doc.root.start <= doc.root.end || doc.root.end == 0, "{text:?}");
                    }
                }
            }
        }
    }
}

const SYNC: &str = "apiVersion: source.toolkit.fluxcd.io/v1
kind: GitRepository
metadata:
  name: flux-system
  namespace: flux-system
spec:
  url: ssh://git@example.org/fleet
---
apiVersion: kustomize.toolkit.fluxcd.io/v1
kind: Kustomization
metadata:
  name: flux-system
  namespace: flux-system
spec:
  path: ./clusters/prod
  sourceRef:
    kind: GitRepository
    name: flux-system
";

const APPS: &str = "apiVersion: kustomize.toolkit.fluxcd.io/v1
kind: Kustomization
metadata:
  name: apps
  namespace: flux-system
spec:
  dependsOn:
    - name: infra
  path: ./apps/prod
  sourceRef:
    kind: GitRepository
    name: flux-system
---
apiVersion: kustomize.toolkit.fluxcd.io/v1
kind: Kustomization
metadata:
  name: infra
  namespace: flux-system
spec:
  path: ./infra/missing
  sourceRef:
    kind: GitRepository
    name: nowhere
";

const RELEASE: &str = "apiVersion: helm.toolkit.fluxcd.io/v2
kind: HelmRelease
metadata:
  name: podinfo
  namespace: apps
spec:
  chart:
    spec:
      chart: podinfo
      sourceRef:
        kind: HelmRepository
        name: podinfo
---
apiVersion: source.toolkit.fluxcd.io/v1
kind: HelmRepository
metadata:
  name: podinfo
  namespace: apps
spec:
  url: https://example.org/charts
---
apiVersion: helm.toolkit.fluxcd.io/v2
kind: HelmRelease
metadata:
  name: lonely
  namespace: apps
spec:
  chart:
    spec:
      chart: lonely
---
apiVersion: helm.toolkit.fluxcd.io/v2
kind: HelmRelease
metadata:
  name: local
  namespace: apps
spec:
  chart:
    spec:
      chart: ./charts/web
      sourceRef:
        kind: GitRepository
        name: flux-system
        namespace: flux-system
";

const KUSTOMIZATION: &str = "apiVersion: kustomize.config.k8s.io/v1beta1
kind: Kustomization
namespace: apps
resources:
  - release.yaml
  - ../base
  - gone.yaml
";

fn flux() -> (Vec<File>, Vec<String>) {
    repository(
        &[
            ("clusters/prod/flux-system/gotk-sync.yaml", SYNC),
            ("clusters/prod/apps.yaml", APPS),
            ("apps/prod/release.yaml", RELEASE),
            ("apps/prod/kustomization.yaml", KUSTOMIZATION),
            ("apps/base/deployment.yaml", "apiVersion: apps/v1\nkind: Deployment\nmetadata:\n  name: web\n"),
            ("charts/web/Chart.yaml", "apiVersion: v2\nname: web\nversion: 1.2.0\n"),
        ],
        &[
            "clusters",
            "clusters/prod",
            "clusters/prod/flux-system",
            "apps",
            "apps/prod",
            "apps/base",
            "charts",
            "charts/web",
        ],
    )
}

#[test]
fn a_flux_repository_is_recognised_with_its_sources_kustomizations_and_releases() {
    let (files, paths) = flux();
    let index = model::build(&files, &paths);
    assert_eq!(index.kinds.iter().copied().collect::<Vec<_>>(), ["flux", "helm", "kustomize"]);
    let ids: Vec<&str> = index.nodes.iter().map(|node| node.id.as_str()).collect();
    for id in [
        "flux:GitRepository/flux-system/flux-system",
        "flux:Kustomization/flux-system/flux-system",
        "flux:Kustomization/flux-system/apps",
        "flux:HelmRelease/apps/podinfo",
        "flux:HelmRepository/apps/podinfo",
        "kustomize:apps/prod",
        "chart:charts/web",
        "path:apps/prod",
        "path:clusters/prod",
    ] {
        assert!(ids.contains(&id), "{id} in {ids:?}");
    }
    let edges = edges(&index);
    for (from, to, kind) in [
        ("flux:Kustomization/flux-system/apps", "flux:GitRepository/flux-system/flux-system", "source"),
        ("flux:Kustomization/flux-system/apps", "path:apps/prod", "path"),
        ("flux:Kustomization/flux-system/apps", "flux:Kustomization/flux-system/infra", "dependsOn"),
        ("flux:HelmRelease/apps/podinfo", "flux:HelmRepository/apps/podinfo", "source"),
        ("flux:HelmRelease/apps/local", "chart:charts/web", "chart"),
        ("path:apps/prod", "flux:HelmRelease/apps/podinfo", "resource"),
        ("kustomize:apps/prod", "flux:HelmRelease/apps/podinfo", "resource"),
        ("kustomize:apps/prod", "path:apps/base", "resource"),
    ] {
        assert!(edges.contains(&(from.to_owned(), to.to_owned(), kind)), "{from} -{kind}-> {to} in {edges:?}");
    }
    let release = index.nodes.iter().find(|node| node.id == "flux:HelmRelease/apps/podinfo").unwrap();
    assert!(text_at(&files, &release.path, release.start, release.end).starts_with("apiVersion: helm.toolkit"));
    let deployment = index.nodes.iter().any(|node| node.path == "apps/base/deployment.yaml");
    assert!(!deployment, "plain Kubernetes objects are not nodes");
}

#[test]
fn broken_references_are_problems_on_the_words_that_name_them() {
    let (files, paths) = flux();
    let index = model::build(&files, &paths);
    let problems: Vec<(String, &str, bool)> = index
        .problems
        .iter()
        .map(|problem| {
            (problem.message.clone(), text_at(&files, &problem.path, problem.start, problem.end), problem.error)
        })
        .collect();
    for expected in [
        ("No GitRepository named nowhere in this repository", "nowhere", true),
        ("No folder ./infra/missing in this repository", "./infra/missing", true),
        ("No file or folder gone.yaml next to this kustomization", "gone.yaml", true),
    ] {
        let expected = (expected.0.to_owned(), expected.1, expected.2);
        assert!(problems.contains(&expected), "{expected:?} in {problems:?}");
    }
    assert!(
        problems
            .iter()
            .any(|(message, text, _)| message.contains("names no repository") && text.contains("chart: lonely")),
        "a HelmRelease without its repository: {problems:?}"
    );
    assert_eq!(problems.len(), 4, "nothing reported falsely: {problems:?}");
}

#[test]
fn references_lead_to_what_they_name() {
    let (files, paths) = flux();
    let index = model::build(&files, &paths);
    let targets: Vec<(&str, &str)> = index
        .references
        .iter()
        .map(|reference| (text_at(&files, &reference.path, reference.start, reference.end), reference.target.as_str()))
        .collect();
    for expected in [
        ("flux-system", "flux:GitRepository/flux-system/flux-system"),
        ("./apps/prod", "path:apps/prod"),
        ("infra", "flux:Kustomization/flux-system/infra"),
        ("podinfo", "flux:HelmRepository/apps/podinfo"),
        ("./charts/web", "chart:charts/web"),
        ("../base", "path:apps/base"),
        ("release.yaml", "flux:HelmRelease/apps/podinfo"),
        ("apps", "flux:Kustomization/flux-system/apps"),
    ] {
        assert!(targets.contains(&expected), "{expected:?} in {targets:?}");
    }
}

#[test]
fn duplicates_and_cycles_are_reported() {
    let twice =
        "apiVersion: source.toolkit.fluxcd.io/v1\nkind: GitRepository\nmetadata:\n  name: same\n  namespace: x\n";
    let loop_a = "apiVersion: kustomize.toolkit.fluxcd.io/v1\nkind: Kustomization\nmetadata:\n  name: a\nspec:\n  dependsOn:\n  - name: b\n---\napiVersion: kustomize.toolkit.fluxcd.io/v1\nkind: Kustomization\nmetadata:\n  name: b\nspec:\n  dependsOn:\n  - name: a\n";
    let (files, paths) = repository(&[("one.yaml", twice), ("two.yaml", twice), ("loop.yaml", loop_a)], &[]);
    let index = model::build(&files, &paths);
    let messages: Vec<&str> = index.problems.iter().map(|problem| problem.message.as_str()).collect();
    assert_eq!(messages.iter().filter(|message| message.contains("more than once")).count(), 2, "{messages:?}");
    assert_eq!(messages.iter().filter(|message| message.contains("depends on itself")).count(), 2, "{messages:?}");
}

const ROOT_APP: &str = "apiVersion: argoproj.io/v1alpha1
kind: Application
metadata:
  name: root
  namespace: argocd
spec:
  project: platform
  source:
    repoURL: https://example.org/fleet.git
    path: apps
  destination:
    server: https://kubernetes.default.svc
";

const CHILD_APP: &str = "apiVersion: argoproj.io/v1alpha1
kind: Application
metadata:
  name: web
  namespace: argocd
spec:
  project: missing-project
  source:
    repoURL: https://example.org/fleet.git
    path: services/web
";

const PROJECT: &str = "apiVersion: argoproj.io/v1alpha1
kind: AppProject
metadata:
  name: platform
  namespace: argocd
";

const SET: &str = "apiVersion: argoproj.io/v1alpha1
kind: ApplicationSet
metadata:
  name: services
  namespace: argocd
spec:
  generators:
  - git:
      repoURL: https://example.org/fleet.git
      directories:
      - path: services/*
  template:
    metadata:
      name: '{{path.basename}}'
    spec:
      project: '{{project}}'
      source:
        path: '{{path}}'
";

#[test]
fn an_argo_cd_repository_shows_its_app_of_apps_projects_and_application_sets() {
    let (files, paths) = repository(
        &[
            ("bootstrap/root.yaml", ROOT_APP),
            ("apps/web.yaml", CHILD_APP),
            ("apps/set.yaml", SET),
            ("projects/platform.yaml", PROJECT),
            ("services/web/deployment.yaml", "kind: Deployment\n"),
            ("services/api/deployment.yaml", "kind: Deployment\n"),
        ],
        &["bootstrap", "apps", "projects", "services", "services/web", "services/api"],
    );
    let index = model::build(&files, &paths);
    assert!(index.kinds.contains("argocd"));
    let edges = edges(&index);
    for (from, to, kind) in [
        ("argocd:Application/argocd/root", "path:apps", "path"),
        ("argocd:Application/argocd/root", "argocd:Application/argocd/web", "child"),
        ("argocd:Application/argocd/root", "argocd:ApplicationSet/argocd/services", "child"),
        ("argocd:Application/argocd/root", "argocd:AppProject/argocd/platform", "project"),
        ("argocd:Application/argocd/web", "path:services/web", "path"),
        ("argocd:ApplicationSet/argocd/services", "path:services/api", "path"),
        ("argocd:ApplicationSet/argocd/services", "path:services/web", "path"),
    ] {
        assert!(edges.contains(&(from.to_owned(), to.to_owned(), kind)), "{from} -{kind}-> {to} in {edges:?}");
    }
    let problems: Vec<(&str, bool)> =
        index.problems.iter().map(|problem| (problem.message.as_str(), problem.error)).collect();
    assert_eq!(
        problems,
        [("No AppProject named missing-project in this repository", false)],
        "templates are not checked"
    );
}

#[test]
fn a_project_without_gitops_files_answers_nothing() {
    let input =
        serde_json::json!({ "files": [{ "path": "a.yaml", "text": "kind: Deployment\n" }], "paths": ["a.yaml"] });
    assert!(crate::index_request(input.to_string().as_bytes()).is_empty());
    let (files, paths) = flux();
    let input = serde_json::json!({
        "files": files.iter().map(|file| serde_json::json!({ "path": file.path, "text": file.text })).collect::<Vec<_>>(),
        "paths": paths,
    });
    let answer: serde_json::Value =
        serde_json::from_slice(&crate::index_request(input.to_string().as_bytes())).unwrap();
    assert!(answer["nodes"].as_array().is_some_and(|nodes| nodes.len() > 5));
    assert!(answer["problems"][0]["severity"] == "error");
    assert!(crate::index_request(b"not json").is_empty());
}

#[test]
fn an_argo_cd_path_of_this_repository_is_an_error_and_another_s_a_warning() {
    let app = |name: &str, url: &str| {
        format!(
            "apiVersion: argoproj.io/v1alpha1\nkind: Application\nmetadata:\n  name: {name}\nspec:\n  source:\n    repoURL: {url}\n    path: gone/{name}\n"
        )
    };
    let (here, there, again) = (
        app("here", "https://example.org/Team/fleet.git"),
        app("there", "https://example.org/team/other"),
        app("again", "ssh://git@example.org/team/fleet"),
    );
    let (files, paths) = repository(&[("a.yaml", &here), ("b.yaml", &there), ("c.yaml", &again)], &[]);
    let remotes = vec!["git@example.org:team/fleet.git".to_owned()];
    let index = model::build_with(&files, &paths, &remotes);
    let severity = |name: &str| {
        index
            .problems
            .iter()
            .find(|problem| problem.message.contains(&format!("gone/{name}")))
            .map(|problem| problem.error)
    };
    assert_eq!(severity("here"), Some(true), "this repository, as https with .git");
    assert_eq!(severity("again"), Some(true), "this repository, as ssh://");
    assert_eq!(severity("there"), Some(false), "another repository");
    assert!(model::same_repository("git@example.org:team/fleet.git", "https://EXAMPLE.org/team/fleet/"));
    assert!(!model::same_repository("https://example.org/team/fleet", "https://example.org/team/fleet2"));
}

const CHART: &str = "apiVersion: v2\nname: web\nversion: 1.2.0\n";
const VALUES: &str = "replicaCount: 2\nimage:\n  repository: nginx\n  tag: \"1.27\"\npodAnnotations: {}\n";
const PROD: &str = "image:\n  tag: \"1.28\"\nextra: on\n";
const DEPLOYMENT: &str = "apiVersion: apps/v1\nkind: Deployment\nspec:\n  replicas: {{ .Values.replicaCount }}\n  image: \"{{ .Values.image.repository }}:{{ .Values.image.tag }}\"\n  annotations: {{ toYaml .Values.podAnnotations.extra }}\n  gone: {{ .Values.image.digest }}\n  shared: {{ $.Values.global.domain }}\n  word: {{ x.Values.nothing }}\n";

fn chart() -> (Vec<File>, Vec<String>) {
    repository(
        &[
            ("charts/web/Chart.yaml", CHART),
            ("charts/web/values.yaml", VALUES),
            ("charts/web/values-prod.yaml", PROD),
            ("charts/web/templates/deployment.yaml", DEPLOYMENT),
            ("charts/web/templates/_helpers.tpl", "{{- define \"web.name\" -}}{{ .Values.image.tag }}{{- end }}"),
        ],
        &["charts", "charts/web", "charts/web/templates"],
    )
}

fn target_text<'a>(index: &Index, files: &'a [File], target: &str) -> (String, &'a str) {
    let node = index.nodes.iter().find(|node| node.id == target).unwrap();
    (node.path.clone(), text_at(files, &node.path, node.start, node.end))
}

#[test]
fn a_values_reference_in_a_template_leads_to_its_key_and_the_key_back_to_its_uses() {
    let (files, paths) = chart();
    let index = model::build(&files, &paths);
    let template = "charts/web/templates/deployment.yaml";
    let at = |text: &str| {
        let start = DEPLOYMENT.find(text).unwrap();
        index.references.iter().find(|reference| reference.path == template && reference.start == start).unwrap()
    };
    let tag = at(".Values.image.tag");
    assert_eq!(&DEPLOYMENT[tag.start..tag.end], ".Values.image.tag");
    assert_eq!(target_text(&index, &files, &tag.target), ("charts/web/values.yaml".to_owned(), "tag"));
    let open = at(".Values.podAnnotations.extra");
    assert_eq!(target_text(&index, &files, &open.target).1, "podAnnotations", "an empty map is filled from outside");
    let in_prod = index
        .references
        .iter()
        .find(|reference| reference.path == "charts/web/values-prod.yaml" && reference.target == tag.target);
    assert!(in_prod.is_some(), "the same key in another values file names the same value");
    let uses = index.references.iter().filter(|reference| reference.target == tag.target).count();
    assert_eq!(uses, 4, "values.yaml, values-prod.yaml, the template and the helper: {:?}", index.references);
    let values = index.nodes.iter().filter(|node| node.kind == "Value").collect::<Vec<_>>();
    assert!(values.iter().all(|node| !node.in_graph), "values keys stay out of the graph");
    assert!(values.iter().any(|node| node.name == "extra" && node.path.ends_with("values-prod.yaml")));
}

#[test]
fn a_values_key_in_no_values_file_is_a_warning_except_globals_and_other_words() {
    let (files, paths) = chart();
    let index = model::build(&files, &paths);
    let warnings: Vec<&str> = index
        .problems
        .iter()
        .filter(|problem| problem.path.ends_with("deployment.yaml"))
        .map(|problem| &DEPLOYMENT[problem.start..problem.end])
        .collect();
    assert_eq!(warnings, [".Values.image.digest"], "{:?}", index.problems);
    assert!(index.problems.iter().all(|problem| !problem.error), "a value may come from --set or -f");
    assert!(index.problems[0].message.contains("image.digest") && index.problems[0].message.contains("web"));
    assert!(
        index.nodes.iter().all(|node| node.path != "charts/web/templates/deployment.yaml"),
        "a template is not an object of the repository"
    );
}

#[test]
fn templates_complete_the_keys_of_their_chart_s_values() {
    let (files, paths) = chart();
    let index = model::build(&files, &paths);
    assert_eq!(index.completions.len(), 1);
    let completion = &index.completions[0];
    assert_eq!(completion.after, ".Values.");
    assert_eq!(completion.files, ["charts/web/templates/deployment.yaml", "charts/web/templates/_helpers.tpl"]);
    let labels: Vec<&str> = completion.items.iter().map(|(label, _)| label.as_str()).collect();
    assert_eq!(labels, ["replicaCount", "image", "image.repository", "image.tag", "podAnnotations", "extra"]);
    assert_eq!(completion.items[5].1, "values-prod.yaml");
    let json = model::to_json(&index);
    assert_eq!(json["completions"][0]["items"][0]["label"], "replicaCount");
    assert_eq!(json["nodes"].as_array().unwrap().iter().filter(|node| node["inGraph"] == false).count(), 6);
}

#[test]
fn a_missing_patch_file_of_a_kustomization_is_an_error() {
    let kustomization = "resources:\n- deployment.yaml\npatches:\n- path: replicas.yaml\n- path: gone.yaml\n- patch: |-\n    - op: remove\npatchesStrategicMerge:\n- memory.yaml\npatchesJson6902:\n- path: gone-json.yaml\n";
    let (files, mut paths) = repository(&[("prod/kustomization.yaml", kustomization)], &["prod"]);
    paths.extend(["prod/deployment.yaml", "prod/replicas.yaml"].map(str::to_owned));
    let index = model::build(&files, &paths);
    let missing: Vec<&str> = index.problems.iter().map(|problem| &kustomization[problem.start..problem.end]).collect();
    assert_eq!(missing, ["gone.yaml", "gone-json.yaml", "memory.yaml"], "{:?}", index.problems);
    assert!(index.problems.iter().all(|problem| problem.error && problem.message.contains("patch")));
}

#[test]
fn a_chart_and_a_local_overlay_offer_a_read_only_preview_and_a_remote_overlay_none() {
    let run = |path: &str, text: &str| -> serde_json::Value {
        serde_json::from_slice(&crate::run_targets_request(format!("{path}\0{text}").as_bytes())).unwrap()
    };
    let chart = run("charts/web/Chart.yaml", CHART);
    assert_eq!(chart[0]["program"], "helm");
    assert_eq!(chart[0]["args"], serde_json::json!(["template", "${fileDir}"]));
    assert_eq!(
        (chart[0]["line"].as_u64(), &chart[0]["output"], &chart[0]["documentType"]),
        (Some(1), &"document".into(), &"yaml".into())
    );
    let folder = run("charts/web/", "charts/web/Chart.yaml\ncharts/web/templates/x.yaml\n");
    assert_eq!(folder[0]["args"], serde_json::json!(["template", "${file}"]));
    assert_eq!(run("charts/", "charts/web/Chart.yaml\n"), serde_json::json!([]), "only the chart's own folder");
    let overlay = run("prod/kustomization.yaml", "resources:\n- ../base\n");
    assert_eq!(overlay[0]["program"], "kustomize");
    assert_eq!(overlay[0]["args"], serde_json::json!(["build", "${fileDir}"]));
    let remote = run("prod/kustomization.yaml", "resources:\n- https://github.com/team/base?ref=v1\n");
    assert_eq!(remote, serde_json::json!([]), "kustomize would fetch it");
    for target in [&chart[0], &folder[0], &overlay[0]] {
        let args = target["args"].to_string();
        for refused in
            ["--dependency-update", "--post-renderer", "--enable-helm", "--enable-exec", "--enable-alpha-plugins"]
        {
            assert!(!args.contains(refused), "{args}");
        }
    }
    assert_eq!(run("deploy.yaml", "kind: Deployment\n"), serde_json::json!([]));
}

#[test]
fn the_manifest_declares_the_two_programs_and_the_files_of_the_previews() {
    let manifest: serde_json::Value = serde_json::from_str(include_str!("../plugin.json")).unwrap();
    assert_eq!(manifest["runs"], serde_json::json!(["helm", "kustomize", "kubectl"]));
    let targets: Vec<&str> = manifest["targetFiles"].as_array().unwrap().iter().filter_map(|v| v.as_str()).collect();
    assert!(targets.contains(&"Chart.yaml") && targets.contains(&"kustomization.yaml"));
    let person_only = manifest["personOnly"].as_array().unwrap();
    assert_eq!(person_only.len(), 1, "previews only read: only kubectl's changing subcommands are the person's");
    assert_eq!(person_only[0]["program"], "kubectl");
    assert!(person_only[0]["subcommands"].as_array().unwrap().iter().any(|word| word == "apply"));
}

const FLUX_STATE: &str = r#"{"kind": "List", "items": [
  {"apiVersion": "kustomize.toolkit.fluxcd.io/v1", "kind": "Kustomization",
   "metadata": {"name": "apps", "namespace": "flux-system"},
   "status": {"lastAppliedRevision": "main@sha1:4f2c", "conditions": [
     {"type": "Ready", "status": "False", "reason": "BuildFailed", "message": "kustomize build failed: missing file\nmore"}]}},
  {"apiVersion": "source.toolkit.fluxcd.io/v1", "kind": "GitRepository",
   "metadata": {"name": "flux-system", "namespace": "flux-system"},
   "status": {"artifact": {"revision": "main@sha1:4f2c"}, "conditions": [{"type": "Ready", "status": "True"}]}},
  {"apiVersion": "helm.toolkit.fluxcd.io/v2", "kind": "HelmRelease",
   "metadata": {"name": "web", "namespace": "team"}, "spec": {"suspend": true},
   "status": {"history": [{"chartVersion": "1.2.0"}]}},
  {"apiVersion": "helm.toolkit.fluxcd.io/v2", "kind": "HelmRelease",
   "metadata": {"name": "web", "namespace": "shop"}, "status": {"conditions": [{"type": "Ready", "status": "Unknown"}]}},
  {"apiVersion": "v1", "kind": "ConfigMap", "metadata": {"name": "other", "namespace": "x"}}
]}"#;

const ARGO_STATE: &str = r#"{"items": [
  {"apiVersion": "argoproj.io/v1alpha1", "kind": "Application", "metadata": {"name": "shop", "namespace": "argocd"},
   "status": {"sync": {"status": "OutOfSync", "revision": "9a1b"}, "health": {"status": "Healthy"},
              "conditions": [{"message": "ComparisonError: path gone"}]}},
  {"apiVersion": "argoproj.io/v1alpha1", "kind": "Application", "metadata": {"name": "api", "namespace": "argocd"},
   "status": {"sync": {"status": "Synced"}, "health": {"status": "Degraded", "message": "Back-off pulling image"}}}
]}"#;

fn state_rows(output: &str) -> Vec<serde_json::Value> {
    let bytes = crate::tool_rows_request(format!("{}\0{output}", crate::cluster::SECTION).as_bytes());
    serde_json::from_slice::<serde_json::Value>(&bytes).unwrap().as_array().unwrap().clone()
}

fn state_of<'a>(rows: &'a [serde_json::Value], id: &str) -> &'a serde_json::Value {
    rows.iter().find(|row| row["id"] == id).unwrap_or_else(|| panic!("{id}: {rows:?}"))
}

#[test]
fn flux_objects_read_their_ready_condition_revision_and_error_from_kubectl_get() {
    let rows = state_rows(FLUX_STATE);
    let apps = state_of(&rows, "flux:Kustomization/flux-system/apps");
    assert_eq!(apps["state"], "error");
    assert_eq!(apps["columns"][0], "False (BuildFailed)");
    assert_eq!(apps["columns"][3], "main@sha1:4f2c");
    assert_eq!(apps["columns"][4], "kustomize build failed: missing file", "one line");
    assert_eq!(
        serde_json::Value::from(apps["columns"].as_array().unwrap()[5..].to_vec()),
        serde_json::json!(["apps", "flux-system", "involvedObject.kind=Kustomization,involvedObject.name=apps"])
    );
    assert_eq!(state_of(&rows, "flux:Kustomization/-/apps")["state"], "error", "also for a file without its namespace");
    assert_eq!(state_of(&rows, "flux:GitRepository/flux-system/flux-system")["state"], "ok");
    let suspended = state_of(&rows, "flux:HelmRelease/team/web");
    assert_eq!(
        (&suspended["state"], &suspended["columns"][0], &suspended["columns"][3]),
        (&"unknown".into(), &"Suspended".into(), &"1.2.0".into())
    );
    assert_eq!(state_of(&rows, "flux:HelmRelease/shop/web")["state"], "progressing");
    assert!(rows.iter().all(|row| row["id"] != "flux:HelmRelease/-/web"), "a name in two namespaces is not guessed");
    assert!(rows.iter().all(|row| !row["id"].as_str().unwrap().contains("ConfigMap")));
}

#[test]
fn argo_cd_applications_read_sync_health_revision_and_message() {
    let rows = state_rows(ARGO_STATE);
    let shop = state_of(&rows, "argocd:Application/argocd/shop");
    assert_eq!(shop["state"], "progressing", "out of sync");
    assert_eq!(
        serde_json::Value::from(shop["columns"].as_array().unwrap()[1..5].to_vec()),
        serde_json::json!(["OutOfSync", "Healthy", "9a1b", "ComparisonError: path gone"])
    );
    let api = state_of(&rows, "argocd:Application/argocd/api");
    assert_eq!((&api["state"], &api["columns"][4]), (&"error".into(), &"Back-off pulling image".into()));
    assert!(state_rows("error: not json").is_empty());
    assert!(
        serde_json::from_slice::<serde_json::Value>(&crate::tool_rows_request(b"other\0{}"))
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn the_state_is_only_read_with_the_kubernetes_window_s_context() {
    let manifest: serde_json::Value = serde_json::from_str(include_str!("../plugin.json")).unwrap();
    let window = &manifest["toolWindows"][0];
    assert_eq!(
        window["windowValues"],
        serde_json::json!([{ "plugin": "kubernetes", "window": "kubernetes", "choice": "context" }])
    );
    let section = &window["sections"][0];
    let mut commands: Vec<&serde_json::Value> = section["state"]["lists"].as_array().unwrap().iter().collect();
    commands.extend(section["actions"].as_array().unwrap());
    for command in commands {
        let args: Vec<&str> = command["args"].as_array().unwrap().iter().filter_map(|arg| arg.as_str()).collect();
        assert_eq!(args[..3], ["--context", "${window.kubernetes.context}", "get"], "{args:?}");
        assert!(
            args.iter().all(|arg| !arg.contains("${") || arg.starts_with("${") && arg.ends_with('}')),
            "whole arguments"
        );
    }
    assert!(section["actions"][0]["args"].to_string().contains("--namespace"));
}
