use std::collections::{BTreeMap, HashMap, HashSet};

use tokio_postgres::Client;
use tokio_postgres::types::{ToSql, Type};

use super::error;
use crate::driver::DbError;
use crate::schema::{
    Catalog, Column, Constraint, ConstraintKind, Dependency, EventTrigger, Extension, Index, IntrospectRequest,
    Introspection, Level, Object, ObjectKind, Partitioning, Policy, Reference, Role, Routine, Rule, Schema,
    SequenceInfo, Snapshot, Tablespace, Trigger, TypeInfo, qualified,
};

type Rows = Vec<Vec<Option<String>>>;

const DATABASES: &str = "SELECT d.datname::text, d.datistemplate::text, shobj_description(d.oid, 'pg_database') \
FROM pg_database d WHERE d.datallowconn ORDER BY 1";

const SCHEMAS: &str = "SELECT n.nspname::text, pg_get_userbyid(n.nspowner)::text, obj_description(n.oid, 'pg_namespace'), \
(n.nspname LIKE 'pg\\_%' OR n.nspname = 'information_schema')::text \
FROM pg_namespace n WHERE n.nspname !~ '^pg_(toast_)?temp_' AND n.nspname <> 'pg_toast' ORDER BY 1";

const FINGERPRINTS: &str = "SELECT n.nspname::text, concat_ws('/', \
(SELECT count(*) || ':' || coalesce(sum(c.xmin::text::bigint), 0) FROM pg_class c WHERE c.relnamespace = n.oid), \
(SELECT count(*) || ':' || coalesce(sum(a.xmin::text::bigint), 0) FROM pg_attribute a JOIN pg_class c ON c.oid = a.attrelid WHERE c.relnamespace = n.oid), \
(SELECT count(*) || ':' || coalesce(sum(d.xmin::text::bigint), 0) FROM pg_attrdef d JOIN pg_class c ON c.oid = d.adrelid WHERE c.relnamespace = n.oid), \
(SELECT count(*) || ':' || coalesce(sum(r.xmin::text::bigint), 0) FROM pg_rewrite r JOIN pg_class c ON c.oid = r.ev_class WHERE c.relnamespace = n.oid), \
(SELECT count(*) || ':' || coalesce(sum(p.xmin::text::bigint), 0) FROM pg_proc p WHERE p.pronamespace = n.oid), \
(SELECT count(*) || ':' || coalesce(sum(o.xmin::text::bigint), 0) FROM pg_constraint o WHERE o.connamespace = n.oid), \
(SELECT count(*) || ':' || coalesce(sum(i.xmin::text::bigint), 0) FROM pg_index i JOIN pg_class c ON c.oid = i.indrelid WHERE c.relnamespace = n.oid), \
(SELECT count(*) || ':' || coalesce(sum(t.xmin::text::bigint), 0) FROM pg_trigger t JOIN pg_class c ON c.oid = t.tgrelid WHERE c.relnamespace = n.oid), \
(SELECT count(*) || ':' || coalesce(sum(p.xmin::text::bigint), 0) FROM pg_policy p JOIN pg_class c ON c.oid = p.polrelid WHERE c.relnamespace = n.oid), \
(SELECT count(*) || ':' || coalesce(sum(t.xmin::text::bigint), 0) FROM pg_type t WHERE t.typnamespace = n.oid), \
(SELECT count(*) || ':' || coalesce(sum(e.xmin::text::bigint), 0) FROM pg_enum e JOIN pg_type t ON t.oid = e.enumtypid WHERE t.typnamespace = n.oid), \
(SELECT count(*) || ':' || coalesce(sum(s.xmin::text::bigint), 0) FROM pg_sequence s JOIN pg_class c ON c.oid = s.seqrelid WHERE c.relnamespace = n.oid), \
(SELECT count(*) || ':' || coalesce(sum(i.xmin::text::bigint), 0) FROM pg_inherits i JOIN pg_class c ON c.oid = i.inhrelid WHERE c.relnamespace = n.oid), \
(SELECT count(*) || ':' || coalesce(sum(d.xmin::text::bigint), 0) FROM pg_description d WHERE d.objoid = n.oid OR d.objoid IN \
(SELECT oid FROM pg_class WHERE relnamespace = n.oid UNION ALL SELECT oid FROM pg_proc WHERE pronamespace = n.oid \
UNION ALL SELECT oid FROM pg_type WHERE typnamespace = n.oid UNION ALL SELECT oid FROM pg_constraint WHERE connamespace = n.oid)), \
n.xmin::text), \
((SELECT count(*) FROM pg_class c WHERE c.relnamespace = n.oid AND c.relkind IN ('r', 'p', 'v', 'm', 'f', 'S')) \
+ (SELECT count(*) FROM pg_proc p WHERE p.pronamespace = n.oid))::text \
FROM pg_namespace n WHERE n.nspname = ANY($1)";

const RELATIONS: &str = "SELECT n.nspname::text, c.relname::text, c.relkind::text, c.oid::text, pg_get_userbyid(c.relowner)::text, \
obj_description(c.oid, 'pg_class'), CASE WHEN c.reltuples >= 0 THEN c.reltuples::bigint::text END, \
CASE WHEN n.nspname = ANY($2) AND c.relkind IN ('r', 'm') THEN pg_total_relation_size(c.oid)::text END, \
pn.nspname::text, pc.relname::text, CASE WHEN c.relispartition THEN pg_get_expr(c.relpartbound, c.oid) END, \
CASE WHEN c.relkind = 'p' THEN pg_get_partkeydef(c.oid) END, c.relrowsecurity::text, array_to_json(c.relacl::text[])::text, \
EXISTS (SELECT 1 FROM pg_depend d WHERE d.classid = 'pg_class'::regclass AND d.objid = c.oid AND d.deptype = 'e')::text, \
CASE WHEN n.nspname = ANY($2) AND c.relkind IN ('v', 'm') THEN pg_get_viewdef(c.oid, true) END, \
(SELECT t.spcname::text FROM pg_tablespace t WHERE t.oid = c.reltablespace) \
FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
LEFT JOIN pg_inherits i ON c.relispartition AND i.inhrelid = c.oid \
LEFT JOIN pg_class pc ON pc.oid = i.inhparent LEFT JOIN pg_namespace pn ON pn.oid = pc.relnamespace \
WHERE n.nspname = ANY($1) AND c.relkind IN ('r', 'p', 'v', 'm', 'f', 'S') ORDER BY 1, 2";

const ROUTINES: &str = "SELECT n.nspname::text, p.proname::text, p.prokind::text, p.oid::text, \
pg_get_function_identity_arguments(p.oid), pg_get_function_result(p.oid), l.lanname::text, pg_get_userbyid(p.proowner)::text, \
obj_description(p.oid, 'pg_proc'), CASE WHEN n.nspname = ANY($2) AND p.prokind <> 'a' THEN pg_get_functiondef(p.oid) END, \
EXISTS (SELECT 1 FROM pg_depend d WHERE d.classid = 'pg_proc'::regclass AND d.objid = p.oid AND d.deptype = 'e')::text, \
array_to_json(p.proacl::text[])::text \
FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace JOIN pg_language l ON l.oid = p.prolang \
WHERE n.nspname = ANY($1) ORDER BY 1, 2, 5";

const SEQUENCES: &str = "SELECT n.nspname::text, c.relname::text, format_type(s.seqtypid, NULL), s.seqstart::text, \
s.seqincrement::text, s.seqmin::text, s.seqmax::text, s.seqcache::text, s.seqcycle::text, \
(SELECT quote_ident(tn.nspname) || '.' || quote_ident(tc.relname) || '.' || quote_ident(a.attname) FROM pg_depend d \
JOIN pg_class tc ON tc.oid = d.refobjid JOIN pg_namespace tn ON tn.oid = tc.relnamespace \
JOIN pg_attribute a ON a.attrelid = d.refobjid AND a.attnum = d.refobjsubid \
WHERE d.classid = 'pg_class'::regclass AND d.objid = c.oid AND d.refclassid = 'pg_class'::regclass AND d.deptype IN ('a', 'i') LIMIT 1) \
FROM pg_sequence s JOIN pg_class c ON c.oid = s.seqrelid JOIN pg_namespace n ON n.oid = c.relnamespace \
WHERE n.nspname = ANY($1) ORDER BY 1, 2";

const TYPES: &str = "SELECT n.nspname::text, t.typname::text, t.typtype::text, t.oid::text, pg_get_userbyid(t.typowner)::text, \
obj_description(t.oid, 'pg_type'), \
array_to_json(ARRAY(SELECT e.enumlabel::text FROM pg_enum e WHERE e.enumtypid = t.oid ORDER BY e.enumsortorder))::text, \
CASE WHEN t.typtype = 'd' THEN format_type(t.typbasetype, t.typtypmod) \
WHEN t.typtype = 'r' THEN (SELECT format_type(r.rngsubtype, NULL) FROM pg_range r WHERE r.rngtypid = t.oid) END, \
t.typnotnull::text, t.typdefault, \
array_to_json(ARRAY(SELECT pg_get_constraintdef(o.oid, true) FROM pg_constraint o WHERE o.contypid = t.oid AND o.contype = 'c' ORDER BY o.conname))::text, \
EXISTS (SELECT 1 FROM pg_depend d WHERE d.classid = 'pg_type'::regclass AND d.objid = t.oid AND d.deptype = 'e')::text \
FROM pg_type t JOIN pg_namespace n ON n.oid = t.typnamespace \
WHERE n.nspname = ANY($1) AND (t.typtype IN ('e', 'd', 'r') \
OR (t.typtype = 'c' AND EXISTS (SELECT 1 FROM pg_class c WHERE c.oid = t.typrelid AND c.relkind = 'c'))) ORDER BY 1, 2";

const COLUMNS: &str = "SELECT n.nspname::text, c.relname::text, a.attnum::text, a.attname::text, \
format_type(a.atttypid, a.atttypmod), (NOT a.attnotnull)::text, pg_get_expr(d.adbin, d.adrelid), a.attidentity::text, \
a.attgenerated::text, CASE WHEN a.attcollation <> 0 AND a.attcollation <> t.typcollation THEN \
(SELECT quote_ident(cn.nspname) || '.' || quote_ident(co.collname) FROM pg_collation co \
JOIN pg_namespace cn ON cn.oid = co.collnamespace WHERE co.oid = a.attcollation) END, col_description(c.oid, a.attnum) \
FROM pg_attribute a JOIN pg_class c ON c.oid = a.attrelid JOIN pg_namespace n ON n.oid = c.relnamespace \
JOIN pg_type t ON t.oid = a.atttypid LEFT JOIN pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum \
WHERE n.nspname = ANY($1) AND ($2::text[] IS NULL OR c.oid::text = ANY($2)) \
AND c.relkind IN ('r', 'p', 'v', 'm', 'f', 'c') AND a.attnum > 0 AND NOT a.attisdropped ORDER BY 1, 2, a.attnum";

const CONSTRAINTS: &str = "SELECT n.nspname::text, c.relname::text, o.conname::text, o.contype::text, \
pg_get_constraintdef(o.oid, true), \
array_to_json(ARRAY(SELECT a.attname::text FROM unnest(o.conkey) WITH ORDINALITY k(num, ord) \
JOIN pg_attribute a ON a.attrelid = o.conrelid AND a.attnum = k.num ORDER BY k.ord))::text, \
rn.nspname::text, rc.relname::text, \
array_to_json(ARRAY(SELECT a.attname::text FROM unnest(o.confkey) WITH ORDINALITY k(num, ord) \
JOIN pg_attribute a ON a.attrelid = o.confrelid AND a.attnum = k.num ORDER BY k.ord))::text, \
o.confupdtype::text, o.confdeltype::text, \
CASE WHEN o.contype IN ('p', 'u', 'x') THEN (SELECT i.relname::text FROM pg_class i WHERE i.oid = o.conindid) END, \
obj_description(o.oid, 'pg_constraint') \
FROM pg_constraint o JOIN pg_class c ON c.oid = o.conrelid JOIN pg_namespace n ON n.oid = c.relnamespace \
LEFT JOIN pg_class rc ON rc.oid = o.confrelid LEFT JOIN pg_namespace rn ON rn.oid = rc.relnamespace \
WHERE n.nspname = ANY($1) AND ($2::text[] IS NULL OR c.oid::text = ANY($2)) AND o.contype IN ('p', 'u', 'f', 'c', 'x') \
ORDER BY 1, 2, 3";

const INDEXES: &str = "SELECT n.nspname::text, c.relname::text, i.relname::text, x.indisunique::text, x.indisprimary::text, \
pg_get_indexdef(x.indexrelid), am.amname::text, \
array_to_json(ARRAY(SELECT pg_get_indexdef(x.indexrelid, k, true) FROM generate_series(1, x.indnkeyatts::int) k))::text, \
(SELECT o.conname::text FROM pg_constraint o WHERE o.conindid = x.indexrelid AND o.conrelid = x.indrelid \
AND o.contype IN ('p', 'u', 'x') LIMIT 1), pg_relation_size(i.oid)::text, obj_description(i.oid, 'pg_class') \
FROM pg_index x JOIN pg_class i ON i.oid = x.indexrelid JOIN pg_class c ON c.oid = x.indrelid \
JOIN pg_namespace n ON n.oid = c.relnamespace JOIN pg_am am ON am.oid = i.relam \
WHERE n.nspname = ANY($1) AND ($2::text[] IS NULL OR c.oid::text = ANY($2)) ORDER BY 1, 2, 3";

const TRIGGERS: &str = "SELECT n.nspname::text, c.relname::text, t.tgname::text, pg_get_triggerdef(t.oid, true), \
(t.tgenabled <> 'D')::text, p.proname::text \
FROM pg_trigger t JOIN pg_class c ON c.oid = t.tgrelid JOIN pg_namespace n ON n.oid = c.relnamespace \
JOIN pg_proc p ON p.oid = t.tgfoid \
WHERE n.nspname = ANY($1) AND ($2::text[] IS NULL OR c.oid::text = ANY($2)) AND NOT t.tgisinternal ORDER BY 1, 2, 3";

const POLICIES: &str = "SELECT n.nspname::text, c.relname::text, p.polname::text, p.polcmd::text, p.polpermissive::text, \
array_to_json(ARRAY(SELECT CASE WHEN r = 0 THEN 'public' ELSE pg_get_userbyid(r)::text END FROM unnest(p.polroles) r))::text, \
pg_get_expr(p.polqual, p.polrelid), pg_get_expr(p.polwithcheck, p.polrelid) \
FROM pg_policy p JOIN pg_class c ON c.oid = p.polrelid JOIN pg_namespace n ON n.oid = c.relnamespace \
WHERE n.nspname = ANY($1) AND ($2::text[] IS NULL OR c.oid::text = ANY($2)) ORDER BY 1, 2, 3";

const RULES: &str = "SELECT n.nspname::text, c.relname::text, r.rulename::text, r.ev_type::text, r.is_instead::text, \
pg_get_ruledef(r.oid, true) \
FROM pg_rewrite r JOIN pg_class c ON c.oid = r.ev_class JOIN pg_namespace n ON n.oid = c.relnamespace \
WHERE n.nspname = ANY($1) AND ($2::text[] IS NULL OR c.oid::text = ANY($2)) AND r.rulename <> '_RETURN' ORDER BY 1, 2, 3";

const DEPENDENCIES: &str = "SELECT n.nspname::text, c.relname::text, 'uses'::text, rn.nspname::text, rc.relname::text, rc.relkind::text \
FROM pg_rewrite r JOIN pg_class c ON c.oid = r.ev_class JOIN pg_namespace n ON n.oid = c.relnamespace \
JOIN pg_depend d ON d.classid = 'pg_rewrite'::regclass AND d.objid = r.oid AND d.refclassid = 'pg_class'::regclass \
JOIN pg_class rc ON rc.oid = d.refobjid JOIN pg_namespace rn ON rn.oid = rc.relnamespace \
WHERE n.nspname = ANY($1) AND ($2::text[] IS NULL OR c.oid::text = ANY($2)) AND rc.oid <> c.oid \
UNION SELECT rn.nspname::text, rc.relname::text, 'usedBy'::text, n.nspname::text, c.relname::text, c.relkind::text \
FROM pg_rewrite r JOIN pg_class c ON c.oid = r.ev_class JOIN pg_namespace n ON n.oid = c.relnamespace \
JOIN pg_depend d ON d.classid = 'pg_rewrite'::regclass AND d.objid = r.oid AND d.refclassid = 'pg_class'::regclass \
JOIN pg_class rc ON rc.oid = d.refobjid JOIN pg_namespace rn ON rn.oid = rc.relnamespace \
WHERE rn.nspname = ANY($1) AND ($2::text[] IS NULL OR rc.oid::text = ANY($2)) AND rc.oid <> c.oid \
UNION SELECT n.nspname::text, c.relname::text, 'uses'::text, pn.nspname::text, p.proname::text, 'function'::text \
FROM pg_rewrite r JOIN pg_class c ON c.oid = r.ev_class JOIN pg_namespace n ON n.oid = c.relnamespace \
JOIN pg_depend d ON d.classid = 'pg_rewrite'::regclass AND d.objid = r.oid AND d.refclassid = 'pg_proc'::regclass \
JOIN pg_proc p ON p.oid = d.refobjid JOIN pg_namespace pn ON pn.oid = p.pronamespace \
WHERE n.nspname = ANY($1) AND ($2::text[] IS NULL OR c.oid::text = ANY($2)) AND pn.nspname <> 'pg_catalog' \
UNION SELECT tn.nspname::text, tc.relname::text, 'uses'::text, sn.nspname::text, sc.relname::text, sc.relkind::text \
FROM pg_depend d JOIN pg_class sc ON sc.oid = d.objid AND sc.relkind = 'S' JOIN pg_namespace sn ON sn.oid = sc.relnamespace \
JOIN pg_class tc ON tc.oid = d.refobjid JOIN pg_namespace tn ON tn.oid = tc.relnamespace \
WHERE d.classid = 'pg_class'::regclass AND d.refclassid = 'pg_class'::regclass AND d.deptype IN ('a', 'i') \
AND tn.nspname = ANY($1) AND ($2::text[] IS NULL OR tc.oid::text = ANY($2)) \
ORDER BY 1, 2, 3, 4, 5";

const EXTENSIONS: &str = "SELECT e.extname::text, e.extversion::text, n.nspname::text, e.extrelocatable::text, \
obj_description(e.oid, 'pg_extension') FROM pg_extension e JOIN pg_namespace n ON n.oid = e.extnamespace ORDER BY 1";

const EVENT_TRIGGERS: &str = "SELECT e.evtname::text, e.evtevent::text, e.evtfoid::regproc::text, e.evtenabled::text, \
pg_get_userbyid(e.evtowner)::text, array_to_json(coalesce(e.evttags, '{}'::text[]))::text, \
obj_description(e.oid, 'pg_event_trigger') FROM pg_event_trigger e ORDER BY 1";

const ROLES: &str = "SELECT r.rolname::text, r.rolcanlogin::text, r.rolsuper::text, r.rolcreatedb::text, \
r.rolcreaterole::text, r.rolreplication::text, r.rolbypassrls::text, r.rolconnlimit::text, r.rolvaliduntil::text, \
array_to_json(ARRAY(SELECT b.rolname::text FROM pg_auth_members m JOIN pg_roles b ON b.oid = m.roleid \
WHERE m.member = r.oid ORDER BY 1))::text, shobj_description(r.oid, 'pg_authid') FROM pg_roles r ORDER BY 1";

const TABLESPACES: &str = "SELECT t.spcname::text, pg_get_userbyid(t.spcowner)::text, pg_tablespace_location(t.oid), \
CASE WHEN has_tablespace_privilege(t.oid, 'CREATE') OR pg_has_role('pg_read_all_stats', 'MEMBER') \
OR t.oid = (SELECT d.dattablespace FROM pg_database d WHERE d.datname = current_database()) \
THEN pg_tablespace_size(t.oid)::text END, array_to_json(coalesce(t.spcoptions, '{}'::text[]))::text, \
shobj_description(t.oid, 'pg_tablespace') FROM pg_tablespace t ORDER BY 1";

const OBJECT_DETAILS: &str = "SELECT c.oid::text, CASE WHEN c.relkind IN ('v', 'm') THEN pg_get_viewdef(c.oid, true) END, \
CASE WHEN c.relkind IN ('r', 'm') THEN pg_total_relation_size(c.oid)::text END \
FROM pg_class c WHERE c.oid::text = ANY($1)";

const ROUTINE_DETAILS: &str =
    "SELECT p.oid::text, pg_get_functiondef(p.oid) FROM pg_proc p WHERE p.oid::text = ANY($1) AND p.prokind <> 'a'";

async fn optional(client: &Client, sql: &str, params: &[(&(dyn ToSql + Sync), Type)]) -> Result<Rows, DbError> {
    Ok(rows(client, sql, params).await.unwrap_or_default())
}

async fn rows(client: &Client, sql: &str, params: &[(&(dyn ToSql + Sync), Type)]) -> Result<Rows, DbError> {
    let rows = client.query_typed(sql, params).await.map_err(|failed| error(&failed))?;
    Ok(rows
        .iter()
        .map(|row| (0..row.len()).map(|index| row.try_get::<_, Option<String>>(index).ok().flatten()).collect())
        .collect())
}

fn cell(row: &[Option<String>], index: usize) -> Option<String> {
    row.get(index).cloned().flatten()
}

fn text(row: &[Option<String>], index: usize) -> String {
    cell(row, index).unwrap_or_default()
}

fn flag(row: &[Option<String>], index: usize) -> bool {
    matches!(cell(row, index).as_deref(), Some("true" | "t"))
}

fn list(row: &[Option<String>], index: usize) -> Vec<String> {
    cell(row, index).and_then(|json| serde_json::from_str::<Vec<String>>(&json).ok()).unwrap_or_default()
}

fn action(code: &str) -> String {
    match code {
        "r" => "restrict",
        "c" => "cascade",
        "n" => "set null",
        "d" => "set default",
        _ => "no action",
    }
    .to_owned()
}

fn system_schema(name: &str) -> bool {
    name.starts_with("pg_") || name == "information_schema"
}

type Key = (String, String);

fn by_relation(rows: Rows) -> HashMap<Key, Vec<Vec<Option<String>>>> {
    let mut grouped: HashMap<Key, Vec<Vec<Option<String>>>> = HashMap::new();
    for row in rows {
        grouped.entry((text(&row, 0), text(&row, 1))).or_default().push(row);
    }
    grouped
}

struct Details {
    columns: HashMap<Key, Vec<Vec<Option<String>>>>,
    constraints: HashMap<Key, Vec<Vec<Option<String>>>>,
    indexes: HashMap<Key, Vec<Vec<Option<String>>>>,
    triggers: HashMap<Key, Vec<Vec<Option<String>>>>,
    policies: HashMap<Key, Vec<Vec<Option<String>>>>,
    rules: HashMap<Key, Vec<Vec<Option<String>>>>,
    dependencies: HashMap<Key, Vec<Vec<Option<String>>>>,
}

impl Details {
    async fn read(client: &Client, schemas: &[String], oids: Option<&[String]>) -> Result<Self, DbError> {
        let schemas = schemas.to_vec();
        let oids: Option<Vec<String>> = oids.map(<[String]>::to_vec);
        let params: [(&(dyn ToSql + Sync), Type); 2] = [(&schemas, Type::TEXT_ARRAY), (&oids, Type::TEXT_ARRAY)];
        let (columns, constraints, indexes, triggers, policies, rules, dependencies) = tokio::try_join!(
            rows(client, COLUMNS, &params),
            rows(client, CONSTRAINTS, &params),
            rows(client, INDEXES, &params),
            rows(client, TRIGGERS, &params),
            rows(client, POLICIES, &params),
            optional(client, RULES, &params),
            optional(client, DEPENDENCIES, &params),
        )?;
        Ok(Self {
            columns: by_relation(columns),
            constraints: by_relation(constraints),
            indexes: by_relation(indexes),
            triggers: by_relation(triggers),
            policies: by_relation(policies),
            rules: by_relation(rules),
            dependencies: by_relation(dependencies),
        })
    }

    fn fill(&mut self, schema: &str, object: &mut Object) {
        let key = (schema.to_owned(), object.name.clone());
        object.columns = self.columns.remove(&key).unwrap_or_default().iter().map(|row| column(row)).collect();
        if object.kind == ObjectKind::Type {
            object.detailed = true;
            return;
        }
        object.constraints =
            self.constraints.remove(&key).unwrap_or_default().iter().filter_map(|row| constraint(row)).collect();
        object.indexes = self
            .indexes
            .remove(&key)
            .unwrap_or_default()
            .iter()
            .map(|row| Index {
                name: text(row, 2),
                unique: flag(row, 3),
                primary: flag(row, 4),
                definition: text(row, 5),
                method: text(row, 6),
                columns: list(row, 7),
                constraint: cell(row, 8),
                size: cell(row, 9).and_then(|size| size.parse().ok()),
                comment: cell(row, 10),
            })
            .collect();
        object.triggers = self
            .triggers
            .remove(&key)
            .unwrap_or_default()
            .iter()
            .map(|row| Trigger {
                name: text(row, 2),
                definition: text(row, 3),
                enabled: flag(row, 4),
                function: text(row, 5),
            })
            .collect();
        object.policies = self
            .policies
            .remove(&key)
            .unwrap_or_default()
            .iter()
            .map(|row| Policy {
                name: text(row, 2),
                command: match text(row, 3).as_str() {
                    "r" => "select",
                    "a" => "insert",
                    "w" => "update",
                    "d" => "delete",
                    _ => "all",
                }
                .to_owned(),
                permissive: flag(row, 4),
                roles: list(row, 5),
                using: cell(row, 6),
                check: cell(row, 7),
            })
            .collect();
        object.rules = self
            .rules
            .remove(&key)
            .unwrap_or_default()
            .iter()
            .map(|row| Rule {
                name: text(row, 2),
                event: match text(row, 3).as_str() {
                    "1" => "select",
                    "2" => "update",
                    "3" => "insert",
                    _ => "delete",
                }
                .to_owned(),
                instead: flag(row, 4),
                definition: text(row, 5),
            })
            .collect();
        object.dependencies = self
            .dependencies
            .remove(&key)
            .unwrap_or_default()
            .iter()
            .map(|row| Dependency {
                direction: text(row, 2),
                schema: text(row, 3),
                name: text(row, 4),
                kind: relation_kind(&text(row, 5)).to_owned(),
            })
            .collect();
        object.detailed = true;
    }
}

fn relation_kind(code: &str) -> &str {
    match code {
        "r" | "p" => "table",
        "v" => "view",
        "m" => "materializedView",
        "f" => "foreignTable",
        "S" => "sequence",
        "c" => "type",
        other => other,
    }
}

fn column(row: &[Option<String>]) -> Column {
    let generated = text(row, 8);
    let default = cell(row, 6);
    let (default, generated) = if generated == "s" { (None, default) } else { (default, None) };
    Column {
        position: cell(row, 2).and_then(|position| position.parse().ok()).unwrap_or_default(),
        name: text(row, 3),
        type_name: text(row, 4),
        nullable: flag(row, 5),
        default,
        identity: match text(row, 7).as_str() {
            "a" => Some("always".to_owned()),
            "d" => Some("byDefault".to_owned()),
            _ => None,
        },
        generated,
        collation: cell(row, 9),
        comment: cell(row, 10),
    }
}

fn constraint(row: &[Option<String>]) -> Option<Constraint> {
    let kind = match text(row, 3).as_str() {
        "p" => ConstraintKind::PrimaryKey,
        "u" => ConstraintKind::Unique,
        "c" => ConstraintKind::Check,
        "x" => ConstraintKind::Exclusion,
        "f" => ConstraintKind::ForeignKey,
        _ => return None,
    };
    let references = (kind == ConstraintKind::ForeignKey).then(|| Reference {
        schema: text(row, 6),
        table: text(row, 7),
        columns: list(row, 8),
        on_update: action(&text(row, 9)),
        on_delete: action(&text(row, 10)),
    });
    Some(Constraint {
        name: text(row, 2),
        kind,
        definition: text(row, 4),
        columns: list(row, 5),
        references,
        index: cell(row, 11),
        comment: cell(row, 12),
    })
}

fn relation(row: &[Option<String>]) -> Option<Object> {
    let kind = match text(row, 2).as_str() {
        "r" | "p" => ObjectKind::Table,
        "v" => ObjectKind::View,
        "m" => ObjectKind::MaterializedView,
        "f" => ObjectKind::ForeignTable,
        "S" => ObjectKind::Sequence,
        _ => return None,
    };
    let mut object = Object::new(kind, text(row, 1));
    object.oid = cell(row, 3).and_then(|oid| oid.parse().ok()).unwrap_or_default();
    object.owner = cell(row, 4);
    object.comment = cell(row, 5);
    object.rows = cell(row, 6).and_then(|rows| rows.parse().ok());
    object.size = cell(row, 7).and_then(|size| size.parse().ok());
    let parent = match (cell(row, 8), cell(row, 9)) {
        (Some(schema), Some(table)) => Some(qualified(&schema, &table)),
        _ => None,
    };
    let key = cell(row, 11);
    if parent.is_some() || key.is_some() {
        object.partitioning = Some(Partitioning { key, parent, bound: cell(row, 10) });
    }
    object.row_security = flag(row, 12);
    object.acl = list(row, 13);
    object.system = flag(row, 14) || system_schema(&text(row, 0));
    object.definition = cell(row, 15);
    object.tablespace = cell(row, 16);
    Some(object)
}

fn routine(row: &[Option<String>]) -> Object {
    let prokind = text(row, 2);
    let kind = if prokind == "p" { ObjectKind::Procedure } else { ObjectKind::Function };
    let mut object = Object::new(kind, text(row, 1));
    object.oid = cell(row, 3).and_then(|oid| oid.parse().ok()).unwrap_or_default();
    object.owner = cell(row, 7);
    object.comment = cell(row, 8);
    object.system = flag(row, 10) || system_schema(&text(row, 0));
    object.acl = list(row, 11);
    object.routine = Some(Routine {
        arguments: text(row, 4),
        result: cell(row, 5).filter(|_| kind == ObjectKind::Function),
        language: text(row, 6),
        kind: match prokind.as_str() {
            "p" => "procedure",
            "a" => "aggregate",
            "w" => "window",
            _ => "function",
        }
        .to_owned(),
        definition: cell(row, 9),
    });
    object.detailed = object.routine.as_ref().is_some_and(|routine| routine.definition.is_some()) || prokind == "a";
    object
}

fn data_type(row: &[Option<String>]) -> Object {
    let mut object = Object::new(ObjectKind::Type, text(row, 1));
    object.oid = cell(row, 3).and_then(|oid| oid.parse().ok()).unwrap_or_default();
    object.owner = cell(row, 4);
    object.comment = cell(row, 5);
    object.system = flag(row, 11) || system_schema(&text(row, 0));
    object.type_info = Some(TypeInfo {
        kind: match text(row, 2).as_str() {
            "e" => "enum",
            "d" => "domain",
            "r" => "range",
            _ => "composite",
        }
        .to_owned(),
        labels: list(row, 6),
        base: cell(row, 7),
        not_null: flag(row, 8),
        default: cell(row, 9),
        checks: list(row, 10),
    });
    object.detailed = true;
    object
}

fn sequence(row: &[Option<String>]) -> SequenceInfo {
    SequenceInfo {
        data_type: text(row, 2),
        start: text(row, 3),
        increment: text(row, 4),
        min: text(row, 5),
        max: text(row, 6),
        cache: text(row, 7),
        cycle: flag(row, 8),
        owned_by: cell(row, 9),
    }
}

fn extension(row: &[Option<String>]) -> Extension {
    Extension {
        name: text(row, 0),
        version: text(row, 1),
        schema: text(row, 2),
        relocatable: flag(row, 3),
        comment: cell(row, 4),
    }
}

fn event_trigger(row: &[Option<String>]) -> EventTrigger {
    EventTrigger {
        name: text(row, 0),
        event: text(row, 1),
        function: text(row, 2),
        enabled: match text(row, 3).as_str() {
            "D" => "disabled",
            "R" => "replica",
            "A" => "always",
            _ => "origin",
        }
        .to_owned(),
        owner: cell(row, 4),
        tags: list(row, 5),
        comment: cell(row, 6),
    }
}

fn role(row: &[Option<String>]) -> Role {
    let name = text(row, 0);
    Role {
        login: flag(row, 1),
        superuser: flag(row, 2),
        create_database: flag(row, 3),
        create_role: flag(row, 4),
        replication: flag(row, 5),
        bypass_row_security: flag(row, 6),
        connection_limit: cell(row, 7).and_then(|limit| limit.parse().ok()).unwrap_or(-1),
        valid_until: cell(row, 8),
        member_of: list(row, 9),
        system: name.starts_with("pg_"),
        comment: cell(row, 10),
        name,
    }
}

fn tablespace(row: &[Option<String>]) -> Tablespace {
    Tablespace {
        name: text(row, 0),
        owner: cell(row, 1),
        location: text(row, 2),
        size: cell(row, 3).and_then(|size| size.parse().ok()),
        options: list(row, 4),
        comment: cell(row, 5),
    }
}

async fn read_schemas(
    client: &Client,
    changed: &[String],
    detailed: &[String],
    names: &BTreeMap<String, (Option<String>, Option<String>, bool)>,
) -> Result<Vec<Schema>, DbError> {
    let changed_param = changed.to_vec();
    let detailed_param = detailed.to_vec();
    let no_oids: Option<Vec<String>> = None;
    let both: [(&(dyn ToSql + Sync), Type); 2] =
        [(&changed_param, Type::TEXT_ARRAY), (&detailed_param, Type::TEXT_ARRAY)];
    let one: [(&(dyn ToSql + Sync), Type); 1] = [(&changed_param, Type::TEXT_ARRAY)];
    let details: [(&(dyn ToSql + Sync), Type); 2] = [(&detailed_param, Type::TEXT_ARRAY), (&no_oids, Type::TEXT_ARRAY)];
    let (relations, routines, sequences, types, columns, constraints, indexes, triggers, policies, rules, dependencies) =
        tokio::try_join!(
            rows(client, RELATIONS, &both),
            rows(client, ROUTINES, &both),
            rows(client, SEQUENCES, &one),
            rows(client, TYPES, &one),
            rows(client, COLUMNS, &details),
            rows(client, CONSTRAINTS, &details),
            rows(client, INDEXES, &details),
            rows(client, TRIGGERS, &details),
            rows(client, POLICIES, &details),
            optional(client, RULES, &details),
            optional(client, DEPENDENCIES, &details),
        )?;
    let mut details = Details {
        columns: by_relation(columns),
        constraints: by_relation(constraints),
        indexes: by_relation(indexes),
        triggers: by_relation(triggers),
        policies: by_relation(policies),
        rules: by_relation(rules),
        dependencies: by_relation(dependencies),
    };
    let detailed: HashSet<&String> = detailed.iter().collect();
    let mut objects: BTreeMap<String, Vec<Object>> = changed.iter().map(|name| (name.clone(), Vec::new())).collect();
    let mut sequences: HashMap<Key, SequenceInfo> =
        sequences.iter().map(|row| ((text(row, 0), text(row, 1)), sequence(row))).collect();
    for row in &relations {
        let schema = text(row, 0);
        let Some(mut object) = relation(row) else { continue };
        if object.kind == ObjectKind::Sequence {
            object.sequence = sequences.remove(&(schema.clone(), object.name.clone()));
            object.detailed = true;
        } else if detailed.contains(&schema) {
            details.fill(&schema, &mut object);
        }
        objects.entry(schema).or_default().push(object);
    }
    for row in &routines {
        objects.entry(text(row, 0)).or_default().push(routine(row));
    }
    for row in &types {
        let schema = text(row, 0);
        let mut object = data_type(row);
        if detailed.contains(&schema) {
            details.fill(&schema, &mut object);
        }
        objects.entry(schema).or_default().push(object);
    }
    Ok(objects
        .into_iter()
        .map(|(name, mut objects)| {
            objects.sort_by_key(|object| (object.kind, object.key()));
            let (owner, comment, system) = names.get(&name).cloned().unwrap_or_default();
            Schema {
                level: if detailed.contains(&name) { Level::Details } else { Level::Names },
                name,
                owner,
                comment,
                system,
                fingerprint: None,
                objects,
            }
        })
        .collect())
}

async fn read_objects(client: &Client, catalog: &mut Catalog, wanted: &[(String, String)]) -> Result<(), DbError> {
    let mut relations: Vec<String> = Vec::new();
    let mut routines: Vec<String> = Vec::new();
    let mut schemas: Vec<String> = Vec::new();
    for (schema, key) in wanted {
        let Some(found) = catalog.schemas.iter().find(|known| known.name == *schema) else { continue };
        for object in found.objects.iter().filter(|object| object.key() == *key || object.name == *key) {
            if object.routine.is_some() {
                routines.push(object.oid.to_string());
            } else {
                relations.push(object.oid.to_string());
            }
            if !schemas.contains(schema) {
                schemas.push(schema.clone());
            }
        }
    }
    let relations_param = relations.clone();
    let routines_param = routines.clone();
    let one: [(&(dyn ToSql + Sync), Type); 1] = [(&relations_param, Type::TEXT_ARRAY)];
    let functions: [(&(dyn ToSql + Sync), Type); 1] = [(&routines_param, Type::TEXT_ARRAY)];
    let (mut details, extra, definitions) = tokio::try_join!(
        Details::read(client, &schemas, Some(&relations)),
        rows(client, OBJECT_DETAILS, &one),
        rows(client, ROUTINE_DETAILS, &functions),
    )?;
    let extra: HashMap<String, Vec<Option<String>>> = extra.into_iter().map(|row| (text(&row, 0), row)).collect();
    let definitions: HashMap<String, Option<String>> =
        definitions.into_iter().map(|row| (text(&row, 0), cell(&row, 1))).collect();
    for schema in &mut catalog.schemas {
        let name = schema.name.clone();
        for object in &mut schema.objects {
            let oid = object.oid.to_string();
            if relations.contains(&oid) && object.routine.is_none() {
                details.fill(&name, object);
                if let Some(row) = extra.get(&oid) {
                    object.definition = cell(row, 1).or(object.definition.take());
                    object.size = cell(row, 2).and_then(|size| size.parse().ok()).or(object.size);
                }
            }
            if let (Some(definition), Some(routine)) = (definitions.get(&oid), object.routine.as_mut()) {
                routine.definition.clone_from(definition);
                object.detailed = true;
            }
        }
    }
    Ok(())
}

pub(super) async fn introspect(
    client: &Client,
    current: &str,
    request: &IntrospectRequest,
    previous: Option<&Snapshot>,
    fingerprinted: bool,
) -> Result<Introspection, DbError> {
    let (databases, schema_rows, extensions, event_triggers, roles, tablespaces) = tokio::try_join!(
        rows(client, DATABASES, &[]),
        rows(client, SCHEMAS, &[]),
        optional(client, EXTENSIONS, &[]),
        optional(client, EVENT_TRIGGERS, &[]),
        optional(client, ROLES, &[]),
        optional(client, TABLESPACES, &[]),
    )?;
    let before = previous.and_then(|snapshot| snapshot.catalog(current));
    let names: BTreeMap<String, (Option<String>, Option<String>, bool)> =
        schema_rows.iter().map(|row| (text(row, 0), (cell(row, 1), cell(row, 2), flag(row, 3)))).collect();
    let earlier = |name: &str| before.and_then(|catalog| catalog.schemas.iter().find(|schema| schema.name == name));
    let mut current_catalog = Catalog {
        name: current.to_owned(),
        current: true,
        schemas_read: true,
        schemas: names
            .iter()
            .map(|(name, (owner, comment, system))| {
                let mut schema = earlier(name).cloned().unwrap_or_default();
                schema.name.clone_from(name);
                schema.owner.clone_from(owner);
                schema.comment.clone_from(comment);
                schema.system = *system;
                schema
            })
            .collect(),
        extensions: extensions.iter().map(|row| extension(row)).collect(),
        event_triggers: event_triggers.iter().map(|row| event_trigger(row)).collect(),
        roles: roles.iter().map(|row| role(row)).collect(),
        tablespaces: tablespaces.iter().map(|row| tablespace(row)).collect(),
        ..Catalog::default()
    };
    let mut read = Vec::new();
    let mut unchanged = 0;
    if request.outline {
    } else if !request.objects.is_empty() && request.schemas.is_empty() {
        read_objects(client, &mut current_catalog, &request.objects).await?;
    } else {
        let candidates: Vec<String> = names
            .iter()
            .filter(|(name, (_, _, system))| {
                if request.schemas.is_empty() {
                    !system || request.system || earlier(name).is_some_and(|schema| schema.level != Level::NotRead)
                } else {
                    request.schemas.contains(name)
                }
            })
            .map(|(name, _)| name.clone())
            .collect();
        let param = candidates.clone();
        let fingerprints: HashMap<String, (String, usize)> = if fingerprinted {
            rows(client, FINGERPRINTS, &[(&param, Type::TEXT_ARRAY)])
                .await?
                .into_iter()
                .map(|row| (text(&row, 0), (text(&row, 1), cell(&row, 2).and_then(|n| n.parse().ok()).unwrap_or(0))))
                .collect()
        } else {
            HashMap::new()
        };
        let changed: Vec<String> = candidates
            .iter()
            .filter(|name| {
                request.force
                    || !fingerprinted
                    || earlier(name).is_none_or(|schema| {
                        schema.level == Level::NotRead
                            || schema.fingerprint.as_ref() != fingerprints.get(*name).map(|(print, _)| print)
                    })
            })
            .cloned()
            .collect();
        unchanged = candidates.len() - changed.len();
        let large = request.large_schema.max(1);
        let detailed: Vec<String> = changed
            .iter()
            .filter(|name| fingerprints.get(*name).is_none_or(|(_, count)| *count <= large))
            .cloned()
            .collect();
        if !changed.is_empty() {
            let fresh = read_schemas(client, &changed, &detailed, &names).await?;
            for mut schema in fresh {
                schema.fingerprint = fingerprints.get(&schema.name).map(|(print, _)| print.clone());
                read.push(format!("{current}.{}", schema.name));
                if let Some(slot) = current_catalog.schemas.iter_mut().find(|known| known.name == schema.name) {
                    *slot = schema;
                }
            }
        }
        if !request.objects.is_empty() {
            read_objects(client, &mut current_catalog, &request.objects).await?;
        }
    }
    let catalogs = databases
        .iter()
        .map(|row| {
            let name = text(row, 0);
            if name == current {
                let mut catalog = current_catalog.clone();
                catalog.system = flag(row, 1);
                catalog.comment = cell(row, 2);
                return catalog;
            }
            let kept = previous.and_then(|snapshot| snapshot.catalog(&name)).cloned();
            Catalog {
                system: flag(row, 1),
                comment: cell(row, 2),
                current: false,
                ..kept.unwrap_or_else(|| Catalog { name: name.clone(), ..Catalog::default() })
            }
        })
        .collect::<Vec<_>>();
    let catalogs = if catalogs.iter().any(|catalog| catalog.current) {
        catalogs
    } else {
        let mut all = catalogs;
        all.push(current_catalog);
        all
    };
    Ok(Introspection { catalogs, read, unchanged })
}
