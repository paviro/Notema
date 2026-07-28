use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use tree_sitter_highlight::{Highlight, HighlightConfiguration, HighlightEvent, Highlighter};

use crate::tui::theme::{Category, Syntax, Theme};

/// Every tree-sitter capture notema recognizes, paired with the theme category
/// it paints.
///
/// `HighlightConfiguration::configure` matches a grammar's capture to the entry
/// here sharing the most dot-separated parts (order-insensitive, ties to the
/// earlier entry); a capture matching nothing is dropped and renders as plain
/// code. `Highlight(i)` indexes straight back into this table, so `style_for`
/// is a lookup rather than a string compare.
///
/// Deliberately unmapped: `embedded`, which bash/js/python wrap around template
/// substitutions — the wrapper stays plain so the interpolated expression's own
/// captures show through.
const SCOPES: &[(&str, Category)] = &[
    ("comment", Category::Comment),
    ("comment.documentation", Category::Comment),
    // nvim-flavored grammars tag comments `@comment @spell`. Tree-sitter keeps
    // the *last* capture on a node, so leaving `spell` unmapped drops the
    // comment's highlight entirely (swift, sql).
    ("spell", Category::Comment),
    ("keyword", Category::Keyword),
    ("keyword.function", Category::Keyword),
    // Both halves of these compounds are recognized names on their own, and a
    // tie is broken by position — spell them out so order can't decide it.
    ("keyword.operator", Category::Keyword),
    ("keyword.type", Category::Keyword),
    ("conditional", Category::Keyword),
    ("repeat", Category::Keyword),
    ("exception", Category::Keyword),
    ("include", Category::Keyword),
    ("storageclass", Category::Keyword),
    ("string", Category::Str),
    ("string.special", Category::Str),
    ("string.escape", Category::StringEscape),
    // rust, python and json spell string escapes `@escape`.
    ("escape", Category::StringEscape),
    ("string.regexp", Category::StringEscape),
    ("string.regex", Category::StringEscape),
    ("character.special", Category::Punctuation),
    ("number", Category::Number),
    ("float", Category::Number),
    ("boolean", Category::Constant),
    ("constant", Category::Constant),
    ("constant.builtin", Category::Constant),
    ("function", Category::Function),
    ("function.builtin", Category::Function),
    ("type", Category::Type),
    ("type.builtin", Category::Type),
    ("constructor", Category::Type),
    ("constructor.builtin", Category::Type),
    ("namespace", Category::Type),
    ("module", Category::Type),
    ("variable", Category::Variable),
    ("variable.builtin", Category::Variable),
    ("variable.member", Category::Variable),
    ("variable.parameter", Category::Variable),
    ("parameter", Category::Variable),
    ("property", Category::Property),
    ("property.builtin", Category::Property),
    ("field", Category::Property),
    ("operator", Category::Operator),
    ("punctuation", Category::Punctuation),
    ("punctuation.bracket", Category::Punctuation),
    ("punctuation.delimiter", Category::Punctuation),
    ("punctuation.special", Category::Punctuation),
    ("attribute", Category::Attribute),
    ("tag", Category::Tag),
    ("label", Category::Label),
    // html's mismatched closing tags — the only capture any grammar routes to
    // the error color.
    ("tag.error", Category::Error),
    ("error", Category::Error),
];

/// The recognized-name slice `configure` wants, projected out of [`SCOPES`] so
/// the two can't fall out of index alignment.
const HIGHLIGHT_NAMES: [&str; SCOPES.len()] = {
    let mut names = [""; SCOPES.len()];
    let mut index = 0;
    while index < SCOPES.len() {
        names[index] = SCOPES[index].0;
        index += 1;
    }
    names
};

/// A grammar notema can highlight. Held as a table rather than a `match` so
/// tests can enumerate the registered set and check every capture it emits.
struct Grammar {
    /// The fence tokens selecting this grammar; the first is canonical.
    aliases: &'static [&'static str],
    language: fn() -> tree_sitter::Language,
    /// Borrowed for grammars shipping a complete query, owned for typescript,
    /// whose bundled query needs the javascript base in front of it.
    highlights: fn() -> Cow<'static, str>,
}

macro_rules! grammar {
    ($aliases:expr, $krate:ident, $query:ident) => {
        Grammar {
            aliases: &$aliases,
            language: || $krate::LANGUAGE.into(),
            highlights: || Cow::Borrowed($krate::$query),
        }
    };
}

const GRAMMARS: &[Grammar] = &[
    grammar!(
        ["bash", "sh", "shell", "zsh"],
        tree_sitter_bash,
        HIGHLIGHT_QUERY
    ),
    grammar!(["css", "scss", "less"], tree_sitter_css, HIGHLIGHTS_QUERY),
    grammar!(["diff", "patch"], tree_sitter_diff, HIGHLIGHTS_QUERY),
    grammar!(["html", "htm"], tree_sitter_html, HIGHLIGHTS_QUERY),
    grammar!(
        ["javascript", "js"],
        tree_sitter_javascript,
        HIGHLIGHT_QUERY
    ),
    grammar!(["json"], tree_sitter_json, HIGHLIGHTS_QUERY),
    grammar!(["python", "py"], tree_sitter_python, HIGHLIGHTS_QUERY),
    Grammar {
        aliases: &["rust", "rs"],
        language: || tree_sitter_rust::LANGUAGE.into(),
        highlights: || {
            Cow::Owned(format!(
                "{RUST_BASE_QUERY}\n{}",
                tree_sitter_rust::HIGHLIGHTS_QUERY
            ))
        },
    },
    grammar!(["sql"], tree_sitter_sequel, HIGHLIGHTS_QUERY),
    grammar!(["swift"], tree_sitter_swift, HIGHLIGHTS_QUERY),
    grammar!(["toml"], tree_sitter_toml_ng, HIGHLIGHTS_QUERY),
    Grammar {
        aliases: &["typescript", "ts"],
        language: || tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        highlights: || typescript_query(false),
    },
    Grammar {
        aliases: &["tsx"],
        language: || tree_sitter_typescript::LANGUAGE_TSX.into(),
        highlights: || typescript_query(true),
    },
    grammar!(["yaml", "yml"], tree_sitter_yaml, HIGHLIGHTS_QUERY),
];

fn language_for(name: &str) -> Option<&'static Grammar> {
    GRAMMARS
        .iter()
        .find(|grammar| grammar.aliases.contains(&name))
}

/// Rust's bundled query captures neither ordinary identifiers nor most
/// operators — only `*`, `&` and `'` — so a rust block would render its
/// variables and every `=`/`+` as plain code. Every other grammar here carries
/// both. This goes *before* the bundled query: tree-sitter keeps the last
/// capture matching a node, so the grammar's own `@function`/`@type`/`@constant`
/// rules still win over the blanket identifier rule.
const RUST_BASE_QUERY: &str = r#"
(identifier) @variable
["=" "+" "-" "*" "/" "%" "==" "!=" "<" ">" "<=" ">=" "&&" "||" "!"
 "->" "=>" "+=" "-=" "*=" "/=" "%=" ".." "..=" "&" "|" "^" "<<" ">>" "?"] @operator
"#;

/// TypeScript's bundled query is only the *delta* over JavaScript — five capture
/// kinds, no comments, strings, numbers or functions — so a `ts` block gets
/// almost no color on its own. Concatenating the JavaScript base restores the
/// rest; the delta goes last because tree-sitter keeps the last capture matching
/// a node, so TypeScript's `@type` beats the base's `@variable`.
///
/// The JSX patterns reference node types that exist only in the tsx grammar, so
/// plain typescript must not get them: `Query::new` would reject the query and
/// the block would lose highlighting altogether.
fn typescript_query(jsx: bool) -> Cow<'static, str> {
    let mut query = String::from(tree_sitter_javascript::HIGHLIGHT_QUERY);
    if jsx {
        query.push('\n');
        query.push_str(tree_sitter_javascript::JSX_HIGHLIGHT_QUERY);
    }
    query.push('\n');
    query.push_str(tree_sitter_typescript::HIGHLIGHTS_QUERY);
    Cow::Owned(query)
}

thread_local! {
    /// Per-language highlight configs, cached because `HighlightConfiguration::new`
    /// recompiles the grammar's query — needless work on every code block, every
    /// frame. Keyed by the normalized fence language; the config depends only on
    /// the grammar, not the theme, so it's safe to reuse across theme changes.
    static CONFIGS: RefCell<HashMap<String, HighlightConfiguration>> =
        RefCell::new(HashMap::new());
}

pub(crate) fn highlight(theme: &Theme, language: &str, code: &str) -> Option<Vec<Line<'static>>> {
    if !theme.syntax().any_color() {
        return None;
    }
    let key = language.trim().to_ascii_lowercase();
    CONFIGS.with(|configs| {
        let mut configs = configs.borrow_mut();
        if !configs.contains_key(&key) {
            let grammar = language_for(&key)?;
            let mut configuration = HighlightConfiguration::new(
                (grammar.language)(),
                "notema",
                &(grammar.highlights)(),
                "",
                "",
            )
            .ok()?;
            configuration.configure(&HIGHLIGHT_NAMES);
            configs.insert(key.clone(), configuration);
        }
        let configuration = configs.get(&key)?;
        let mut highlighter = Highlighter::new();
        let events = highlighter
            .highlight(configuration, code.as_bytes(), None, |_| None)
            .ok()?;

        let syntax = theme.syntax();
        let plain = theme.md_code();
        let mut active = Vec::new();
        let mut lines = vec![Line::default()];
        for event in events {
            match event.ok()? {
                HighlightEvent::Source { start, end } => {
                    // Innermost *colored* scope, not innermost scope: a category
                    // the theme leaves unset would otherwise punch a hole in a
                    // colored ancestor, e.g. json nests its escape sequences
                    // inside `(string) @string`.
                    let style = active
                        .iter()
                        .rev()
                        .find_map(|index| style_for(*index, syntax, plain))
                        .unwrap_or(plain);
                    // Tree-sitter byte offsets are char-aligned in practice, but a
                    // non-boundary slice would panic mid-draw and take down the TUI;
                    // fall back to unhighlighted rendering instead.
                    push_source(&mut lines, code.get(start..end)?, style);
                }
                HighlightEvent::HighlightStart(Highlight(index)) => active.push(index),
                HighlightEvent::HighlightEnd => {
                    active.pop();
                }
            }
        }
        Some(lines)
    })
}

fn push_source(lines: &mut Vec<Line<'static>>, source: &str, style: Style) {
    for (index, part) in source.split('\n').enumerate() {
        if index > 0 {
            lines.push(Line::default());
        }
        if !part.is_empty() {
            lines
                .last_mut()
                .expect("syntax output always has a line")
                .spans
                .push(Span::styled(part.to_string(), style));
        }
    }
}

/// The style a highlight index paints, derived from the plain code style so the
/// theme's code background and emphasis survive. `None` means the theme left
/// that category unset and the run keeps whatever style encloses it.
fn style_for(index: usize, syntax: Syntax, plain: Style) -> Option<Style> {
    let category = SCOPES.get(index)?.1;
    let color = category.color(syntax);
    if color == Color::Reset {
        // An unset key means "leave this category alone", not "reset the
        // foreground" — the latter would also throw away md_code's background.
        return None;
    }
    Some(plain.fg(color).add_modifier(modifier_for(category)))
}

/// The emphasis a category carries on top of hue: comments lean back, keywords
/// carry weight. Everything else is hue alone.
const fn modifier_for(category: Category) -> Modifier {
    match category {
        Category::Comment => Modifier::ITALIC,
        Category::Keyword => Modifier::BOLD,
        _ => Modifier::empty(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::theme::{Mode, parse_theme};

    /// Captures notema drops on purpose, so the coverage test below stays a
    /// statement of intent rather than a list of everything that happens to work.
    const INTENTIONALLY_UNMAPPED: &[&str] = &["embedded"];

    /// A theme painting each category a distinct color, so a span's foreground
    /// names the category that produced it.
    fn test_theme() -> Theme {
        let table: String = Category::ALL
            .iter()
            .enumerate()
            .map(|(index, category)| format!("{} = \"#{:02x}0000\"\n", category.key(), index + 1))
            .collect();
        parse_theme(
            &format!("[markdown]\ncode = \"#ffffff\"\n[markdown.syntax]\n{table}"),
            Mode::Dark,
        )
        .unwrap()
    }

    fn category_of(style: Style) -> Option<Category> {
        match style.fg {
            Some(Color::Rgb(index, 0, 0)) if index >= 1 => {
                Category::ALL.get(usize::from(index) - 1).copied()
            }
            _ => None,
        }
    }

    fn spans(theme: &Theme, language: &str, code: &str) -> Vec<(String, Style)> {
        highlight(theme, language, code)
            .unwrap_or_else(|| panic!("{language} produced no highlighting"))
            .into_iter()
            .flat_map(|line| line.spans)
            .map(|span| (span.content.to_string(), span.style))
            .collect()
    }

    /// The category painting the first span whose trimmed text is `needle`.
    fn category_at(language: &str, code: &str, needle: &str) -> Option<Category> {
        let theme = test_theme();
        let (_, style) = spans(&theme, language, code)
            .into_iter()
            .find(|(text, _)| text.trim() == needle)
            .unwrap_or_else(|| panic!("{language} produced no span for {needle:?}"));
        category_of(style)
    }

    /// Mirrors `HighlightConfiguration::configure`: a capture resolves to the
    /// recognized name sharing the most dot-separated parts, ties to the first.
    fn recognized(capture: &str) -> Option<Category> {
        let parts: Vec<&str> = capture.split('.').collect();
        let mut best: Option<(usize, Category)> = None;
        for (name, category) in SCOPES {
            let length = name.split('.').count();
            let matches = name.split('.').all(|part| parts.contains(&part));
            if matches && best.is_none_or(|(best_length, _)| length > best_length) {
                best = Some((length, *category));
            }
        }
        best.map(|(_, category)| category)
    }

    #[test]
    fn highlight_names_mirror_the_scope_table() {
        assert_eq!(HIGHLIGHT_NAMES.len(), SCOPES.len());
        for (index, (name, _)) in SCOPES.iter().enumerate() {
            assert_eq!(HIGHLIGHT_NAMES[index], *name);
        }
        // A repeated name is silently dead — `configure` only ever picks the first.
        for (index, (name, _)) in SCOPES.iter().enumerate() {
            assert!(
                !SCOPES[..index].iter().any(|(seen, _)| seen == name),
                "{name} is listed twice"
            );
        }
    }

    #[test]
    fn every_registered_grammar_builds_a_configuration() {
        // A query that fails to compile degrades to *no* highlighting via the
        // `.ok()?` in `highlight`, so nothing else would catch it. This is the
        // tripwire for grammar-crate bumps and for the typescript concatenation.
        for grammar in GRAMMARS {
            let query = (grammar.highlights)();
            HighlightConfiguration::new((grammar.language)(), "notema", &query, "", "")
                .unwrap_or_else(|err| panic!("{:?} query failed: {err}", grammar.aliases[0]));
        }
    }

    #[test]
    fn every_capture_the_grammars_emit_maps_to_a_category() {
        for grammar in GRAMMARS {
            let query = (grammar.highlights)();
            let configuration =
                HighlightConfiguration::new((grammar.language)(), "notema", &query, "", "")
                    .unwrap();
            for capture in configuration.query.capture_names() {
                if capture.starts_with('_') || INTENTIONALLY_UNMAPPED.contains(capture) {
                    continue;
                }
                assert!(
                    recognized(capture).is_some(),
                    "{} emits @{capture}, which no category claims",
                    grammar.aliases[0]
                );
            }
        }
    }

    #[test]
    fn an_unset_category_renders_as_the_plain_code_style() {
        let theme = parse_theme(
            "[markdown]\ncode = { fg = \"white\", bg = \"#202020\" }\n\
             [markdown.syntax]\nkeyword = \"#ff0000\"",
            Mode::Dark,
        )
        .unwrap();
        let spans = spans(&theme, "rust", "// note\nfn f() {}");
        let plain = theme.md_code();
        // Unset means "leave alone": the comment keeps md_code's background and
        // picks up none of the category's italic.
        let (_, comment) = spans.iter().find(|(text, _)| text == "// note").unwrap();
        assert_eq!(*comment, plain);
        // A set category tints on top of md_code rather than replacing it.
        let (_, keyword) = spans.iter().find(|(text, _)| text == "fn").unwrap();
        assert_eq!(
            *keyword,
            plain
                .fg(Color::Rgb(0xff, 0, 0))
                .add_modifier(Modifier::BOLD)
        );
    }

    #[test]
    fn an_uncolored_inner_scope_keeps_its_colored_ancestor() {
        // json nests escape sequences inside `(string) @string`, so an unset
        // `string_escape` must not punch a hole in the surrounding string.
        let theme = parse_theme(
            "[markdown]\ncode = \"#ffffff\"\n[markdown.syntax]\nstring = \"#00ff00\"",
            Mode::Dark,
        )
        .unwrap();
        let string = Color::Rgb(0, 0xff, 0);
        for (text, style) in spans(&theme, "json", "{\"a\": \"x\\ny\"}") {
            if text.contains("\\n") {
                assert_eq!(style.fg, Some(string), "escape lost its enclosing string");
            }
        }
    }

    #[test]
    fn escape_sequences_use_the_string_escape_color() {
        // rust, python and json spell these `@escape`, not `@string.escape`.
        assert_eq!(
            category_at("rust", "let s = \"a\\nb\";", "\\n"),
            Some(Category::StringEscape)
        );
        assert_eq!(
            category_at("python", "s = \"a\\nb\"", "\\n"),
            Some(Category::StringEscape)
        );
    }

    #[test]
    fn spell_tagged_comments_still_highlight() {
        // Both grammars tag comments `@comment @spell`, and tree-sitter keeps the
        // last capture — an unmapped `spell` drops the highlight entirely.
        assert_eq!(
            category_at("swift", "// note\nlet x = 1", "// note"),
            Some(Category::Comment)
        );
        assert_eq!(
            category_at("sql", "-- note\nSELECT 1;", "-- note"),
            Some(Category::Comment)
        );
    }

    #[test]
    fn typescript_blocks_highlight_more_than_the_delta_query() {
        // The bundled typescript query alone carries no comments or strings.
        let code = "// note\nconst greeting: string = \"hi\";";
        assert_eq!(category_at("ts", code, "// note"), Some(Category::Comment));
        assert_eq!(category_at("ts", code, "\"hi\""), Some(Category::Str));
        assert_eq!(category_at("tsx", code, "// note"), Some(Category::Comment));
    }

    #[test]
    fn tsx_highlights_jsx_tags() {
        // The jsx patterns reference nodes only the tsx grammar has.
        assert_eq!(
            category_at("tsx", "const a = <div>hi</div>;", "div"),
            Some(Category::Tag)
        );
    }

    #[test]
    fn rust_identifiers_and_operators_are_themed() {
        // The bundled rust query captures neither, so these used to fall through
        // to the plain code ink while every other language coloured them.
        let code = "let total = count + 1;";
        assert_eq!(category_at("rust", code, "total"), Some(Category::Variable));
        assert_eq!(category_at("rust", code, "="), Some(Category::Operator));
        assert_eq!(category_at("rust", code, "+"), Some(Category::Operator));
        // The grammar's own rules still win over the blanket identifier rule.
        assert_eq!(
            category_at("rust", "fn parse(x: u8) -> Vec<u8> {}", "parse"),
            Some(Category::Function)
        );
        assert_eq!(
            category_at("rust", "fn parse(x: u8) -> Vec<u8> {}", "Vec"),
            Some(Category::Type)
        );
    }

    #[test]
    fn sql_maps_its_nonstandard_captures() {
        // sequel predates the canonical capture names, so it spells column
        // references `@field` and aliases `@variable`; `@field` used to drop
        // straight through to plain code.
        let query = "SELECT price, name AS label FROM t WHERE price > 1.5;";
        assert_eq!(category_at("sql", query, "price"), Some(Category::Property));
        assert_eq!(category_at("sql", query, "label"), Some(Category::Variable));
        assert_eq!(category_at("sql", query, ">"), Some(Category::Operator));
        // `1.5` reads as a string: sequel guards `@float`/`@number` with Lua
        // patterns (`%d`), which tree-sitter's regex `#match?` never satisfies.
        assert_eq!(category_at("sql", query, "1.5"), Some(Category::Str));
    }

    #[test]
    fn a_theme_with_no_syntax_colors_skips_the_highlighter() {
        let plain = parse_theme("[markdown]\ncode = \"#ffffff\"", Mode::Dark).unwrap();
        assert!(highlight(&plain, "rust", "fn f() {}").is_none());
        // An unregistered fence language falls back the same way.
        assert!(highlight(&test_theme(), "brainfuck", "+++").is_none());
    }
}
