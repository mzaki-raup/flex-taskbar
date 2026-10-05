//! Operations on the nested category tree. Categories are addressed by id; every
//! operation walks the tree, so nesting depth is unbounded.

use crate::config::Category;

pub fn max_id(cats: &[Category]) -> u64 {
    cats.iter().map(|c| c.id.max(max_id(&c.children))).max().unwrap_or(0)
}

/// Index path from the root list to the category, e.g. `[2, 0]` is the first
/// child of the third root category.
pub fn index_path(cats: &[Category], id: u64) -> Option<Vec<usize>> {
    for (i, c) in cats.iter().enumerate() {
        if c.id == id {
            return Some(vec![i]);
        }
        if let Some(mut rest) = index_path(&c.children, id) {
            rest.insert(0, i);
            return Some(rest);
        }
    }
    None
}

fn list_at_mut<'a>(cats: &'a mut Vec<Category>, parent_path: &[usize]) -> &'a mut Vec<Category> {
    let mut list = cats;
    for &i in parent_path {
        list = &mut list[i].children;
    }
    list
}

pub fn find(cats: &[Category], id: u64) -> Option<&Category> {
    for c in cats {
        if c.id == id {
            return Some(c);
        }
        if let Some(found) = find(&c.children, id) {
            return Some(found);
        }
    }
    None
}

pub fn find_mut(cats: &mut [Category], id: u64) -> Option<&mut Category> {
    for c in cats {
        if c.id == id {
            return Some(c);
        }
        if let Some(found) = find_mut(&mut c.children, id) {
            return Some(found);
        }
    }
    None
}

pub fn parent_of(cats: &[Category], id: u64) -> Option<u64> {
    let path = index_path(cats, id)?;
    if path.len() < 2 {
        return None;
    }
    let mut list = cats;
    let mut parent = None;
    for &i in &path[..path.len() - 1] {
        parent = Some(list[i].id);
        list = &list[i].children;
    }
    parent
}

/// Adds `cat` as the last child of `parent` (or as a root category). Returns false
/// if the parent doesn't exist.
pub fn add(cats: &mut Vec<Category>, parent: Option<u64>, cat: Category) -> bool {
    match parent {
        None => {
            cats.push(cat);
            true
        }
        Some(pid) => match find_mut(cats, pid) {
            Some(p) => {
                p.children.push(cat);
                true
            }
            None => false,
        },
    }
}

pub fn remove(cats: &mut Vec<Category>, id: u64) -> Option<Category> {
    let path = index_path(cats, id)?;
    let (last, parent) = path.split_last()?;
    Some(list_at_mut(cats, parent).remove(*last))
}

/// Moves a category one place up (`delta = -1`) or down (`delta = 1`) among its
/// siblings.
pub fn move_sibling(cats: &mut Vec<Category>, id: u64, delta: isize) -> bool {
    let Some(path) = index_path(cats, id) else { return false };
    let (&last, parent) = path.split_last().unwrap();
    let list = list_at_mut(cats, parent);
    let target = last as isize + delta;
    if target < 0 || target as usize >= list.len() {
        return false;
    }
    list.swap(last, target as usize);
    true
}

/// Makes the category the last child of its previous sibling.
pub fn indent(cats: &mut Vec<Category>, id: u64) -> bool {
    let Some(path) = index_path(cats, id) else { return false };
    let (&last, parent) = path.split_last().unwrap();
    if last == 0 {
        return false;
    }
    let list = list_at_mut(cats, parent);
    let cat = list.remove(last);
    list[last - 1].children.push(cat);
    true
}

/// Moves the category out of its parent, placing it right after the parent.
pub fn outdent(cats: &mut Vec<Category>, id: u64) -> bool {
    let Some(path) = index_path(cats, id) else { return false };
    if path.len() < 2 {
        return false;
    }
    let (&last, parent_path) = path.split_last().unwrap();
    let (&parent_idx, grand_path) = parent_path.split_last().unwrap();
    let grand = list_at_mut(cats, grand_path);
    let cat = grand[parent_idx].children.remove(last);
    grand.insert(parent_idx + 1, cat);
    true
}

/// Moves an app one place up or down inside a category.
pub fn move_app(cats: &mut [Category], cat_id: u64, app_id: &str, delta: isize) -> bool {
    let Some(cat) = find_mut(cats, cat_id) else { return false };
    let Some(pos) = cat.apps.iter().position(|a| a == app_id) else { return false };
    let target = pos as isize + delta;
    if target < 0 || target as usize >= cat.apps.len() {
        return false;
    }
    cat.apps.swap(pos, target as usize);
    true
}

/// Appends the app to the category unless it's already there.
pub fn add_app(cats: &mut [Category], cat_id: u64, app_id: &str) -> bool {
    let Some(cat) = find_mut(cats, cat_id) else { return false };
    if cat.apps.iter().any(|a| a == app_id) {
        return false;
    }
    cat.apps.push(app_id.to_string());
    true
}

pub fn remove_app(cats: &mut [Category], cat_id: u64, app_id: &str) -> bool {
    let Some(cat) = find_mut(cats, cat_id) else { return false };
    let before = cat.apps.len();
    cat.apps.retain(|a| a != app_id);
    cat.apps.len() != before
}

pub fn remove_app_everywhere(cats: &mut [Category], app_id: &str) {
    for c in cats {
        c.apps.retain(|a| a != app_id);
        remove_app_everywhere(&mut c.children, app_id);
    }
}

/// Every (app id, "Parent › Child" path) pair, for search and display.
pub fn app_paths(cats: &[Category]) -> Vec<(String, String)> {
    fn walk(cats: &[Category], prefix: &str, out: &mut Vec<(String, String)>) {
        for c in cats {
            let path = if prefix.is_empty() { c.name.clone() } else { format!("{prefix} › {}", c.name) };
            for a in &c.apps {
                out.push((a.clone(), path.clone()));
            }
            walk(&c.children, &path, out);
        }
    }
    let mut out = Vec::new();
    walk(cats, "", &mut out);
    out
}

/// Calls `f(category, depth)` for every category in display order.
pub fn walk(cats: &[Category], f: &mut impl FnMut(&Category, usize)) {
    fn go(cats: &[Category], depth: usize, f: &mut impl FnMut(&Category, usize)) {
        for c in cats {
            f(c, depth);
            go(&c.children, depth + 1, f);
        }
    }
    go(cats, 0, f);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cat(id: u64, name: &str, children: Vec<Category>) -> Category {
        Category { id, name: name.into(), children, ..Default::default() }
    }

    fn sample() -> Vec<Category> {
        vec![
            cat(1, "Dev", vec![cat(2, "Editors", vec![cat(3, "Vim", vec![])]), cat(4, "Tools", vec![])]),
            cat(5, "Games", vec![]),
        ]
    }

    fn names(cats: &[Category]) -> Vec<String> {
        let mut out = Vec::new();
        walk(cats, &mut |c, d| out.push(format!("{}{}", "-".repeat(d), c.name)));
        out
    }

    #[test]
    fn find_works_at_any_depth() {
        let t = sample();
        assert_eq!(find(&t, 3).unwrap().name, "Vim");
        assert_eq!(index_path(&t, 3), Some(vec![0, 0, 0]));
        assert_eq!(parent_of(&t, 3), Some(2));
        assert_eq!(parent_of(&t, 5), None);
        assert!(find(&t, 99).is_none());
        assert_eq!(max_id(&t), 5);
    }

    #[test]
    fn add_and_remove_nested() {
        let mut t = sample();
        assert!(add(&mut t, Some(3), cat(6, "Plugins", vec![])));
        assert_eq!(index_path(&t, 6), Some(vec![0, 0, 0, 0]));
        assert!(!add(&mut t, Some(42), cat(7, "x", vec![])));
        let removed = remove(&mut t, 2).unwrap();
        assert_eq!(removed.children[0].children[0].name, "Plugins");
        assert_eq!(names(&t), vec!["Dev", "-Tools", "Games"]);
    }

    #[test]
    fn move_up_and_down_within_siblings() {
        let mut t = sample();
        assert!(move_sibling(&mut t, 4, -1));
        assert_eq!(names(&t), vec!["Dev", "-Tools", "-Editors", "--Vim", "Games"]);
        assert!(!move_sibling(&mut t, 4, -1));
        assert!(!move_sibling(&mut t, 5, 1));
    }

    #[test]
    fn indent_and_outdent() {
        let mut t = sample();
        assert!(indent(&mut t, 5)); // Games under Dev
        assert_eq!(parent_of(&t, 5), Some(1));
        assert!(!indent(&mut t, 1)); // first root has no previous sibling
        assert!(outdent(&mut t, 3)); // Vim out of Editors, after Editors
        assert_eq!(names(&t), vec!["Dev", "-Editors", "-Vim", "-Tools", "-Games"]);
        assert!(outdent(&mut t, 5));
        assert!(!outdent(&mut t, 5));
        assert_eq!(names(&t), vec!["Dev", "-Editors", "-Vim", "-Tools", "Games"]);
    }

    #[test]
    fn app_membership() {
        let mut t = sample();
        assert!(add_app(&mut t, 3, "a"));
        assert!(!add_app(&mut t, 3, "a"));
        assert!(add_app(&mut t, 3, "b"));
        assert!(add_app(&mut t, 5, "a"));
        assert!(move_app(&mut t, 3, "b", -1));
        assert_eq!(find(&t, 3).unwrap().apps, vec!["b", "a"]);
        assert_eq!(
            app_paths(&t),
            vec![
                ("b".to_string(), "Dev › Editors › Vim".to_string()),
                ("a".to_string(), "Dev › Editors › Vim".to_string()),
                ("a".to_string(), "Games".to_string()),
            ]
        );
        remove_app_everywhere(&mut t, "a");
        assert!(app_paths(&t).iter().all(|(a, _)| a != "a"));
        assert!(remove_app(&mut t, 3, "b"));
        assert!(!remove_app(&mut t, 3, "b"));
    }
}
