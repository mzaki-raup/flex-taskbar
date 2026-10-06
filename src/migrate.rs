//! One-time import from the previous .NET FlexTaskbar, which kept
//! `categories.json` (a flat list linked by `ParentId`) and `applications.json`
//! (each app pointing at one `CategoryId`) under `%APPDATA%\FlexTaskbar\`.
//!
//! Every categorized old app becomes a custom app with its exact launch command,
//! so nothing depends on matching old ids against the new app scan.

use crate::config::{Category, Config, CustomApp};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "PascalCase")]
struct OldCategory {
    id: String,
    name: String,
    parent_id: Option<String>,
    sort_order: i64,
    custom_icon_path: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "PascalCase")]
struct OldApp {
    name: String,
    executable_path: String,
    /// 0 = executable, 1 = URL, 2 = shell command; may also be serialized as a string.
    launch_kind: Value,
    arguments: String,
    working_directory: Option<String>,
    category_id: Option<String>,
    category_sort_order: i64,
    run_as_administrator: bool,
    custom_icon_path: Option<String>,
}

/// An icon file the caller should copy into the data folder.
#[derive(Debug, PartialEq)]
pub enum IconCopy {
    Category { id: u64, source: String },
    App { id: String, source: String },
}

#[derive(Debug, Default, PartialEq)]
pub struct ImportSummary {
    pub categories: usize,
    pub apps: usize,
    pub icons: Vec<IconCopy>,
}

pub fn import(categories_json: &str, apps_json: &str, cfg: &mut Config) -> Result<ImportSummary, String> {
    let old_cats: Vec<OldCategory> =
        serde_json::from_str(categories_json).map_err(|e| format!("categories.json: {e}"))?;
    let old_apps: Vec<OldApp> = if apps_json.trim().is_empty() {
        Vec::new()
    } else {
        serde_json::from_str(apps_json).map_err(|e| format!("applications.json: {e}"))?
    };

    let mut summary = ImportSummary::default();
    let mut new_ids: HashMap<&str, u64> = HashMap::new();
    for c in &old_cats {
        new_ids.insert(c.id.as_str(), cfg.alloc_id());
    }

    // Apps per old category id, in their old order.
    let mut apps_by_cat: HashMap<&str, Vec<&OldApp>> = HashMap::new();
    for a in &old_apps {
        if let Some(cid) = a.category_id.as_deref()
            && new_ids.contains_key(cid)
            && !a.executable_path.trim().is_empty()
        {
            apps_by_cat.entry(cid).or_default().push(a);
        }
    }
    for list in apps_by_cat.values_mut() {
        list.sort_by_key(|a| a.category_sort_order);
    }

    fn build(
        parent: Option<&str>,
        old_cats: &[OldCategory],
        new_ids: &HashMap<&str, u64>,
        apps_by_cat: &HashMap<&str, Vec<&OldApp>>,
        cfg: &mut Config,
        summary: &mut ImportSummary,
        depth: usize,
    ) -> Vec<Category> {
        if depth > 64 {
            return Vec::new(); // a ParentId cycle in a hand-edited file
        }
        let mut level: Vec<&OldCategory> = old_cats
            .iter()
            .filter(|c| match (parent, c.parent_id.as_deref()) {
                (None, None) => true,
                (None, Some(p)) => !new_ids.contains_key(p), // orphan → root
                (Some(want), Some(p)) => want == p,
                (Some(_), None) => false,
            })
            .collect();
        level.sort_by_key(|c| c.sort_order);

        let mut out = Vec::new();
        for oc in level {
            let id = new_ids[oc.id.as_str()];
            if let Some(src) = oc.custom_icon_path.as_ref().filter(|p| !p.is_empty()) {
                summary.icons.push(IconCopy::Category { id, source: src.clone() });
            }
            let mut apps = Vec::new();
            for oa in apps_by_cat.get(oc.id.as_str()).map(|v| v.as_slice()).unwrap_or(&[]) {
                let app_id = format!("custom:{}", cfg.alloc_id());
                if let Some(src) = oa.custom_icon_path.as_ref().filter(|p| !p.is_empty()) {
                    summary.icons.push(IconCopy::App { id: app_id.clone(), source: src.clone() });
                }
                let is_exe = matches!(&oa.launch_kind, Value::Number(n) if n.as_i64() == Some(0))
                    || oa.launch_kind.as_str() == Some("Executable")
                    || oa.launch_kind.is_null();
                cfg.custom_apps.push(CustomApp {
                    id: app_id.clone(),
                    name: oa.name.clone(),
                    target: oa.executable_path.clone(),
                    args: if is_exe { oa.arguments.clone() } else { String::new() },
                    working_dir: if is_exe { oa.working_directory.clone().unwrap_or_default() } else { String::new() },
                    run_as_admin: oa.run_as_administrator,
                });
                apps.push(app_id);
                summary.apps += 1;
            }
            summary.categories += 1;
            let children = build(Some(&oc.id), old_cats, new_ids, apps_by_cat, cfg, summary, depth + 1);
            out.push(Category { id, name: oc.name.clone(), icon: None, apps, children, hotkey: None });
        }
        out
    }

    let roots = build(None, &old_cats, &new_ids, &apps_by_cat, cfg, &mut summary, 0);
    cfg.categories.extend(roots);
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree;

    const CATS: &str = r#"[
        {"Id":"a","Name":"Dev","ParentId":null,"SortOrder":1},
        {"Id":"b","Name":"Games","ParentId":null,"SortOrder":0,"CustomIconPath":"C:\\icons\\g.png"},
        {"Id":"c","Name":"Editors","ParentId":"a","SortOrder":0},
        {"Id":"d","Name":"Orphan","ParentId":"zzz","SortOrder":5}
    ]"#;

    const APPS: &str = r#"[
        {"Id":"1","Name":"Code","ExecutablePath":"C:\\code.exe","LaunchKind":0,"Arguments":"--new-window","WorkingDirectory":"C:\\","CategoryId":"c","CategorySortOrder":1},
        {"Id":"2","Name":"Vim","ExecutablePath":"C:\\vim.exe","LaunchKind":0,"CategoryId":"c","CategorySortOrder":0,"CustomIconPath":"C:\\icons\\v.ico"},
        {"Id":"3","Name":"Discord","ExecutablePath":"shell:AppsFolder\\discord!App","LaunchKind":2,"Arguments":"ignored","CategoryId":"b"},
        {"Id":"4","Name":"Uncategorized","ExecutablePath":"C:\\x.exe","LaunchKind":0}
    ]"#;

    #[test]
    fn rebuilds_tree_and_apps_in_order() {
        let mut cfg = Config::default();
        let s = import(CATS, APPS, &mut cfg).unwrap();
        assert_eq!(s.categories, 4);
        assert_eq!(s.apps, 3);

        let roots: Vec<&str> = cfg.categories.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(roots, vec!["Games", "Dev", "Orphan"]);
        let editors = &cfg.categories[1].children[0];
        assert_eq!(editors.name, "Editors");
        let editor_names: Vec<&str> = editors.apps.iter().map(|id| cfg.custom_app(id).unwrap().name.as_str()).collect();
        assert_eq!(editor_names, vec!["Vim", "Code"]);

        let code = cfg.custom_apps.iter().find(|a| a.name == "Code").unwrap();
        assert_eq!(code.args, "--new-window");
        let discord = cfg.custom_apps.iter().find(|a| a.name == "Discord").unwrap();
        assert_eq!(discord.args, "");

        assert_eq!(s.icons.len(), 2);
        assert!(matches!(&s.icons[0], IconCopy::Category { source, .. } if source.ends_with("g.png")));
    }

    #[test]
    fn imported_ids_do_not_collide_with_existing_ones() {
        let mut cfg = Config::default();
        cfg.categories.push(Category { id: 3, name: "Mine".into(), ..Default::default() });
        import(CATS, APPS, &mut cfg).unwrap();
        let mut ids = Vec::new();
        tree::walk(&cfg.categories, &mut |c, _| ids.push(c.id));
        let mut dedup = ids.clone();
        dedup.sort();
        dedup.dedup();
        assert_eq!(ids.len(), dedup.len());
        assert_eq!(cfg.categories[0].name, "Mine");
    }

    #[test]
    fn parent_cycle_does_not_hang() {
        let cats = r#"[{"Id":"x","Name":"X","ParentId":"y"},{"Id":"y","Name":"Y","ParentId":"x"}]"#;
        let mut cfg = Config::default();
        let s = import(cats, "", &mut cfg).unwrap();
        assert_eq!(s.categories, 0);
    }

    #[test]
    fn bad_json_is_an_error_not_a_panic() {
        let mut cfg = Config::default();
        assert!(import("nope", "", &mut cfg).is_err());
    }
}
