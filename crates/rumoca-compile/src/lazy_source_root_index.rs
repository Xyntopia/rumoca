use indexmap::{IndexMap, IndexSet};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct LazySourceRootIndex {
    uri_to_classes: IndexMap<String, Vec<LazyClassEntry>>,
    class_to_uris: IndexMap<String, IndexSet<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LazyClassEntry {
    pub uri: String,
    pub name: String,
    pub qualified_name: String,
    pub class_type: String,
    pub partial: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LazyClassTreeNode {
    pub name: String,
    pub qualified_name: String,
    pub class_type: String,
    pub partial: bool,
    pub children: Vec<LazyClassTreeNode>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct LazySourceRootIndexSummary {
    pub file_count: usize,
    pub class_count: usize,
}

#[derive(Debug, Clone)]
struct ClassDeclaration {
    name: String,
    class_type: String,
    partial: bool,
}

#[derive(Debug, Clone)]
struct OpenClass {
    name: String,
    qualified_name: String,
}

#[derive(Debug, Clone, Default)]
struct MutableTreeNode {
    name: String,
    qualified_name: String,
    class_type: Option<String>,
    partial: bool,
    children: IndexMap<String, MutableTreeNode>,
}

impl LazySourceRootIndex {
    pub fn from_sources<'a, I>(sources: I) -> Self
    where
        I: IntoIterator<Item = (&'a str, &'a str)>,
    {
        let mut index = Self::default();
        for (uri, source) in sources {
            index.add_source(uri, source);
        }
        index
    }

    pub fn summary(&self) -> LazySourceRootIndexSummary {
        LazySourceRootIndexSummary {
            file_count: self.uri_to_classes.len(),
            class_count: self.class_to_uris.len(),
        }
    }

    pub fn classes_for_uri(&self, uri: &str) -> Option<&[LazyClassEntry]> {
        self.uri_to_classes.get(uri).map(Vec::as_slice)
    }

    pub fn uris_for_class(&self, qualified_name: &str) -> Vec<String> {
        self.class_to_uris
            .get(qualified_name)
            .map(|uris| uris.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub fn class_tree(&self) -> Vec<LazyClassTreeNode> {
        let mut roots = IndexMap::<String, MutableTreeNode>::new();
        for entries in self.uri_to_classes.values() {
            for entry in entries {
                insert_tree_entry(&mut roots, entry);
            }
        }
        roots.into_values().map(freeze_tree_node).collect()
    }

    fn add_source(&mut self, uri: &str, source: &str) {
        let entries = scan_class_entries(uri, source);
        for entry in &entries {
            self.class_to_uris
                .entry(entry.qualified_name.clone())
                .or_default()
                .insert(uri.to_string());
        }
        if !entries.is_empty() {
            self.uri_to_classes.insert(uri.to_string(), entries);
        }
    }
}

fn scan_class_entries(uri: &str, source: &str) -> Vec<LazyClassEntry> {
    let mut entries = Vec::new();
    let sanitized_source = sanitize_source(source);
    let mut within = parse_within_prefix(&sanitized_source);
    let mut open_classes = Vec::<OpenClass>::new();

    for line in sanitized_source.lines() {
        let tokens = modelica_identifier_tokens(line);
        if tokens.is_empty() {
            continue;
        }
        if is_end_statement(&tokens) {
            close_class(&tokens, &mut open_classes);
            continue;
        }
        let Some(declaration) = class_declaration_from_tokens(&tokens) else {
            continue;
        };
        let qualified_name = qualified_child_name(&within, &open_classes, &declaration.name);
        entries.push(LazyClassEntry {
            uri: uri.to_string(),
            name: declaration.name.clone(),
            qualified_name: qualified_name.clone(),
            class_type: declaration.class_type,
            partial: declaration.partial,
        });
        open_classes.push(OpenClass {
            name: declaration.name,
            qualified_name,
        });
        if within.is_empty() {
            within = Vec::new();
        }
    }

    entries
}

fn sanitize_source(source: &str) -> String {
    let mut output = String::with_capacity(source.len());
    let mut chars = source.chars().peekable();
    let mut block_depth = 0usize;
    let mut in_string = false;
    while let Some(ch) = chars.next() {
        if in_string {
            if ch == '\\' {
                output.push(' ');
                if chars.next().is_some() {
                    output.push(' ');
                }
            } else if ch == '"' {
                in_string = false;
                output.push(' ');
            } else if ch == '\n' {
                output.push('\n');
            } else {
                output.push(' ');
            }
            continue;
        }
        if block_depth > 0 {
            if ch == '/' && chars.peek() == Some(&'*') {
                chars.next();
                block_depth += 1;
            } else if ch == '*' && chars.peek() == Some(&'/') {
                chars.next();
                block_depth = block_depth.saturating_sub(1);
            } else if ch == '\n' {
                output.push('\n');
            } else {
                output.push(' ');
            }
            continue;
        }
        if ch == '"' {
            in_string = true;
            output.push(' ');
        } else if ch == '/' && chars.peek() == Some(&'*') {
            chars.next();
            block_depth = 1;
            output.push(' ');
        } else if ch == '/' && chars.peek() == Some(&'/') {
            chars.next();
            for next in chars.by_ref() {
                if next == '\n' {
                    output.push('\n');
                    break;
                }
                output.push(' ');
            }
        } else {
            output.push(ch);
        }
    }
    output
}

fn parse_within_prefix(source: &str) -> Vec<String> {
    for line in source.lines() {
        let tokens = modelica_identifier_tokens(line);
        if tokens.first().is_some_and(|token| token == "within") {
            return tokens
                .iter()
                .skip(1)
                .filter(|token| *token != ".")
                .cloned()
                .collect();
        }
        if tokens.iter().any(|token| is_class_keyword(token)) {
            return Vec::new();
        }
    }
    Vec::new()
}

fn modelica_identifier_tokens(source: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut chars = source.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            current.push(ch);
            continue;
        }
        if !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
        }
        if ch == '\'' {
            let mut quoted = String::new();
            while let Some(next) = chars.next() {
                if next == '\'' {
                    break;
                }
                quoted.push(next);
            }
            if !quoted.is_empty() {
                tokens.push(quoted);
            }
            continue;
        }
        if ch == '.' {
            tokens.push(".".to_string());
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

fn is_end_statement(tokens: &[String]) -> bool {
    tokens.first().is_some_and(|token| token == "end") && tokens.len() >= 2
}

fn close_class(tokens: &[String], open_classes: &mut Vec<OpenClass>) {
    let end_name = &tokens[1];
    if let Some(position) = open_classes
        .iter()
        .rposition(|open_class| &open_class.name == end_name)
    {
        open_classes.truncate(position);
    }
}

fn class_declaration_from_tokens(tokens: &[String]) -> Option<ClassDeclaration> {
    let mut partial = false;
    for (index, token) in tokens.iter().enumerate() {
        if token == "partial" {
            partial = true;
        }
        if token == "operator" {
            if let Some(name) = operator_record_name(tokens, index) {
                return Some(ClassDeclaration {
                    name,
                    class_type: "record".to_string(),
                    partial,
                });
            }
            if let Some(name) = operator_function_name(tokens, index) {
                return Some(ClassDeclaration {
                    name,
                    class_type: "function".to_string(),
                    partial,
                });
            }
            let name = operator_scope_name(tokens, index)?;
            return Some(ClassDeclaration {
                name,
                class_type: token.clone(),
                partial,
            });
        }
        if !is_class_keyword(token) {
            continue;
        }
        let name = declaration_name_after_keyword(tokens, index)?;
        return Some(ClassDeclaration {
            name,
            class_type: token.clone(),
            partial,
        });
    }
    None
}

fn operator_record_name(tokens: &[String], keyword_index: usize) -> Option<String> {
    if tokens.get(keyword_index + 1)? != "record" {
        return None;
    }
    tokens.get(keyword_index + 2).cloned()
}

fn operator_function_name(tokens: &[String], keyword_index: usize) -> Option<String> {
    if tokens.get(keyword_index + 1)? != "function" {
        return None;
    }
    tokens.get(keyword_index + 2).cloned()
}

fn operator_scope_name(tokens: &[String], keyword_index: usize) -> Option<String> {
    let next = tokens.get(keyword_index + 1)?;
    if next == "record" || next == "function" {
        return None;
    }
    Some(next.clone())
}

fn declaration_name_after_keyword(tokens: &[String], keyword_index: usize) -> Option<String> {
    let next = tokens.get(keyword_index + 1)?;
    if next == "operator" {
        tokens.get(keyword_index + 2).cloned()
    } else {
        Some(next.clone())
    }
}

fn is_class_keyword(token: &str) -> bool {
    matches!(
        token,
        "block" | "class" | "connector" | "function" | "model" | "package" | "record" | "type"
    )
}

fn qualified_child_name(within: &[String], open_classes: &[OpenClass], class_name: &str) -> String {
    if let Some(parent) = open_classes.last() {
        return format!("{}.{}", parent.qualified_name, class_name);
    }
    if within.is_empty() {
        class_name.to_string()
    } else {
        format!("{}.{}", within.join("."), class_name)
    }
}

fn insert_tree_entry(roots: &mut IndexMap<String, MutableTreeNode>, entry: &LazyClassEntry) {
    let mut children = roots;
    let parts = entry.qualified_name.split('.').collect::<Vec<_>>();
    let mut prefix = String::new();
    for (index, part) in parts.iter().enumerate() {
        if !prefix.is_empty() {
            prefix.push('.');
        }
        prefix.push_str(part);
        let is_leaf = index + 1 == parts.len();
        let node = children
            .entry((*part).to_string())
            .or_insert_with(|| MutableTreeNode {
                name: (*part).to_string(),
                qualified_name: prefix.clone(),
                class_type: None,
                partial: false,
                children: IndexMap::new(),
            });
        if is_leaf {
            node.class_type = Some(entry.class_type.clone());
            node.partial = entry.partial;
        } else if node.class_type.is_none() {
            node.class_type = Some("package".to_string());
        }
        children = &mut node.children;
    }
}

fn freeze_tree_node(node: MutableTreeNode) -> LazyClassTreeNode {
    LazyClassTreeNode {
        name: node.name,
        qualified_name: node.qualified_name,
        class_type: node.class_type.unwrap_or_else(|| "package".to_string()),
        partial: node.partial,
        children: node.children.into_values().map(freeze_tree_node).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lazy_source_root_index_scans_package_tree_without_parsing() {
        let sources = [
            (
                "Modelica/package.mo",
                r#"
                within ;
                package Modelica
                  package Blocks
                    partial block Base
                    end Base;
                    model Constant
                    end Constant;
                  end Blocks;
                end Modelica;
                "#,
            ),
            (
                "Modelica/Electrical/Analog/package.mo",
                r#"
                within Modelica.Electrical;
                package Analog
                  model Resistor
                  end Resistor;
                end Analog;
                "#,
            ),
        ];

        let index = LazySourceRootIndex::from_sources(sources);

        assert_eq!(index.summary().file_count, 2);
        assert_eq!(index.summary().class_count, 6);
        assert_eq!(
            index.uris_for_class("Modelica.Blocks.Constant"),
            vec!["Modelica/package.mo".to_string()]
        );
        let tree = index.class_tree();
        assert_eq!(tree[0].qualified_name, "Modelica");
        assert_eq!(tree[0].children[0].qualified_name, "Modelica.Blocks");
        assert_eq!(
            tree[0].children[1].children[0].children[0].qualified_name,
            "Modelica.Electrical.Analog.Resistor"
        );
    }

    #[test]
    fn lazy_source_root_index_ignores_comments_and_strings() {
        let index = LazySourceRootIndex::from_sources([(
            "A.mo",
            r#"
            within ;
            // model Commented end Commented;
            package A
              annotation(Documentation(info="model NotReal end NotReal;"));
              model Real
              end Real;
            end A;
            "#,
        )]);

        assert!(index.uris_for_class("A.Commented").is_empty());
        assert!(index.uris_for_class("A.NotReal").is_empty());
        assert_eq!(index.uris_for_class("A.Real"), vec!["A.mo".to_string()]);
    }

    #[test]
    fn lazy_source_root_index_preserves_operator_hierarchy_without_docstring_leaks() {
        let index = LazySourceRootIndex::from_sources([(
            "Complex.mo",
            r#"
            within ;
            operator record Complex
              encapsulated operator 'constructor' "Constructor"
                function fromReal
                  annotation(Documentation(info="<html>
            <p>This function returns a Complex number.</p>
            </html>"));
                end fromReal;
              end 'constructor';

              encapsulated operator function '0' "Zero-element"
                annotation(Documentation(info="<html>
            <p>This function returns the zero element.</p>
            </html>"));
              end '0';

              encapsulated operator '-'
                function negate
                end negate;

                function subtract
                end subtract;
              end '-';

              encapsulated operator '*'
                function multiply
                end multiply;

                function scalarProduct
                  output String s="";
                algorithm
                  for i in 1:2 loop
                    s := s + "x";
                  end for;
                  annotation(Documentation(info="<html>
            <p>This function returns the scalar product.</p>
            </html>"));
                end scalarProduct;
              end '*';

              encapsulated operator function '+'
                annotation(Documentation(info="<html>
            <p>This function returns the sum.</p>
            </html>"));
              end '+';

              encapsulated operator function 'String'
                output String s="";
              algorithm
                if true then
                  s := "Complex";
                end if;
              end 'String';
            end Complex;
            "#,
        )]);

        assert_eq!(
            index.uris_for_class("Complex.constructor"),
            vec!["Complex.mo".to_string()]
        );
        assert_eq!(
            index.uris_for_class("Complex.constructor.fromReal"),
            vec!["Complex.mo".to_string()]
        );
        assert_eq!(
            index.uris_for_class("Complex.0"),
            vec!["Complex.mo".to_string()]
        );
        assert_eq!(
            index.uris_for_class("Complex.-"),
            vec!["Complex.mo".to_string()]
        );
        assert_eq!(
            index.uris_for_class("Complex.-.negate"),
            vec!["Complex.mo".to_string()]
        );
        assert_eq!(
            index.uris_for_class("Complex.-.subtract"),
            vec!["Complex.mo".to_string()]
        );
        assert_eq!(
            index.uris_for_class("Complex.*"),
            vec!["Complex.mo".to_string()]
        );
        assert_eq!(
            index.uris_for_class("Complex.*.multiply"),
            vec!["Complex.mo".to_string()]
        );
        assert_eq!(
            index.uris_for_class("Complex.*.scalarProduct"),
            vec!["Complex.mo".to_string()]
        );
        assert_eq!(
            index.uris_for_class("Complex.+"),
            vec!["Complex.mo".to_string()]
        );
        assert_eq!(
            index.uris_for_class("Complex.String"),
            vec!["Complex.mo".to_string()]
        );

        for bogus_name in [
            "Complex.function",
            "Complex.constructor.s",
            "Complex.constructor.fromReal.returns",
            "Complex.*.scalarProduct.returns",
            "Complex.*.s",
            "Complex.containing",
        ] {
            assert!(
                index.uris_for_class(bogus_name).is_empty(),
                "unexpected leaked node: {bogus_name}"
            );
        }

        let tree = index.class_tree();
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].qualified_name, "Complex");
        let constructor = tree[0]
            .children
            .iter()
            .find(|child| child.qualified_name == "Complex.constructor")
            .expect("Complex.constructor should exist as an operator scope");
        assert!(
            constructor
                .children
                .iter()
                .any(|child| child.qualified_name == "Complex.constructor.fromReal"),
            "fromReal should remain nested under Complex.constructor"
        );
        let minus = tree[0]
            .children
            .iter()
            .find(|child| child.qualified_name == "Complex.-")
            .expect("Complex.- should exist as an operator scope");
        assert!(
            minus
                .children
                .iter()
                .any(|child| child.qualified_name == "Complex.-.negate"),
            "negate should remain nested under Complex.-"
        );
        assert!(
            minus
                .children
                .iter()
                .any(|child| child.qualified_name == "Complex.-.subtract"),
            "subtract should remain nested under Complex.-"
        );
    }
}
