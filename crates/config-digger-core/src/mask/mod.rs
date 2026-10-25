//! Mask: finds the Secrets in an Entry's value and masks them, so that no
//! Secret ever leaves the core.
//!
//! On the common server model, env and header values, a URL's userinfo and
//! query values, and args after a token-like flag are Secrets; a location's
//! definition marks more fields, other fields that hold a URL, and fields
//! that hold a variable name. The fields any location that reads the same
//! file marks are Secrets too, so that every Agent's view of the file masks
//! them. A
//! reference in the Agent's interpolation syntax is not a Secret: a value
//! made only of references is shown as it is, and in a mixed value only the
//! literal text beyond a recognised prefix is masked.

mod diff;

pub(crate) use diff::{Side, diff, diff_parked};

use crate::definitions::blocks::Interpolate;
use crate::definitions::{LocationDefinition, SecretFields};
use serde_json::Value;

/// What a Secret is shown as, whatever its length.
pub(crate) const MASK: &str = "••••";

/// Schemes a masked value keeps, matched without regard to case.
const SCHEMES: &[&str] = &["Bearer ", "Basic "];

/// Token-type prefixes a masked value keeps after any scheme. A longer
/// prefix comes before a shorter one it starts with.
const TOKEN_PREFIXES: &[&str] = &[
    "github_pat_",
    "ghp_",
    "gho_",
    "ghu_",
    "ghs_",
    "ghr_",
    "glpat-",
    "sk-ant-",
    "sk-proj-",
    "sk_live_",
    "sk_test_",
    "rk_live_",
    "sk-",
    "xoxb-",
    "xoxp-",
    "xoxa-",
    "xoxe-",
    "xapp-",
    "npm_",
    "pypi-",
    "hf_",
    "AKIA",
];

/// The words of a flag that make its value a Secret, such as `--api-key`.
const TOKEN_WORDS: &[&str] = &[
    "token",
    "secret",
    "password",
    "passwd",
    "pass",
    "pwd",
    "key",
    "apikey",
    "auth",
    "authorization",
    "credential",
    "credentials",
    "bearer",
    "pat",
    "header",
    "cookie",
];

/// How a flag's name ends when its value is a Secret, such as `--apikey`.
const TOKEN_ENDINGS: &[&str] = &["token", "key", "secret", "password"];

/// An Entry's value with every Secret masked.
pub(crate) struct Masked {
    pub value: Value,
    /// The key paths below the Entry whose values were masked, in the order
    /// they appear.
    pub key_paths: Vec<Vec<String>>,
}

/// An Entry of `location` with every Secret masked.
pub(crate) fn entry(location: &LocationDefinition, value: &Value) -> Masked {
    entry_read_by(location, &[], value)
}

/// An Entry of `location` with every Secret masked, including those that
/// `reading`, the locations that read its file, mark.
pub(crate) fn entry_read_by(
    location: &LocationDefinition,
    reading: &[&LocationDefinition],
    value: &Value,
) -> Masked {
    let rules = Rules::new(location, reading);
    let mut masked = Masked {
        value: value.clone(),
        key_paths: Vec::new(),
    };
    rules.walk(&mut masked.value, &mut Vec::new(), &mut masked.key_paths);
    masked
}

/// An Entry whose location is not known, with every text in it masked as a
/// Secret, since nothing says where its Secrets are.
pub(crate) fn every_text(value: &Value) -> Masked {
    let mut masked = Masked {
        value: value.clone(),
        key_paths: Vec::new(),
    };
    text_leaves(value, &mut Vec::new(), &mut masked.key_paths);
    for key_path in &masked.key_paths {
        let leaf = key_path
            .iter()
            .try_fold(&mut masked.value, |value, key| match value {
                Value::Object(object) => object.get_mut(key),
                Value::Array(array) => array.get_mut(key.parse::<usize>().ok()?),
                _ => None,
            })
            .expect("a text's key path is in the value");
        *leaf = Value::from(MASK);
    }
    masked
}

/// The key paths below `value`, which is at `at`, of every text in it.
fn text_leaves(value: &Value, at: &mut Vec<String>, found: &mut Vec<Vec<String>>) {
    match value {
        Value::String(_) => found.push(at.clone()),
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                at.push(index.to_string());
                text_leaves(item, at, found);
                at.pop();
            }
        }
        Value::Object(object) => {
            for (key, item) in object {
                at.push(key.clone());
                text_leaves(item, at, found);
                at.pop();
            }
        }
        _ => {}
    }
}

/// The Secrets a location's definition marks outside its Entries in a file
/// whose whole value is `document`: each one's key path from the root of the
/// file and its masked value.
pub(crate) fn file_fields(
    location: &LocationDefinition,
    document: &Value,
) -> Vec<(Vec<String>, Value)> {
    let rules = Rules::new(location, &[]);
    let mut found = Vec::new();
    for field in &location.secrets.file_fields {
        collect(document, field, &mut Vec::new(), &mut found);
    }
    let mut secrets = Vec::new();
    for (mut path, value) in found {
        let mut value = value.clone();
        let mut masked = Vec::new();
        rules.mask_leaves(&mut value, &mut Vec::new(), &mut masked);
        for relative in masked {
            let secret = relative
                .iter()
                .try_fold(&value, |value, key| match value {
                    Value::Object(object) => object.get(key),
                    Value::Array(array) => array.get(key.parse::<usize>().ok()?),
                    _ => None,
                })
                .cloned()
                .expect("a masked key path is in the value");
            let depth = path.len();
            path.extend(relative);
            secrets.push((path.clone(), secret));
            path.truncate(depth);
        }
    }
    let mut encrypted = Vec::new();
    rules.encrypted_leaves(document, &mut Vec::new(), &mut encrypted);
    for (path, secret) in encrypted {
        if !secrets.iter().any(|(known, _)| *known == path) {
            secrets.push((path, secret));
        }
    }
    secrets
}

/// The values at a field's key path, where `*` is any key or index.
fn collect<'v>(
    value: &'v Value,
    field: &[String],
    path: &mut Vec<String>,
    found: &mut Vec<(Vec<String>, &'v Value)>,
) {
    let Some((key, rest)) = field.split_first() else {
        found.push((path.clone(), value));
        return;
    };
    let children: Vec<(String, &Value)> = match value {
        Value::Object(object) => object
            .iter()
            .filter(|(name, _)| key == "*" || *name == key)
            .map(|(name, child)| (name.clone(), child))
            .collect(),
        Value::Array(array) => array
            .iter()
            .enumerate()
            .filter(|(index, _)| key == "*" || index.to_string() == *key)
            .map(|(index, child)| (index.to_string(), child))
            .collect(),
        _ => return,
    };
    for (name, child) in children {
        path.push(name);
        collect(child, rest, path, found);
        path.pop();
    }
}

/// Where Secrets are in the Entries of one location.
struct Rules<'d> {
    location: &'d LocationDefinition,
    /// What `location` and each location that reads its file mark as
    /// Secrets. Only `location`'s own variable names and interpolation
    /// syntax count, since those are how its Agent reads the Entry.
    secrets: Vec<&'d SecretFields>,
}

impl<'d> Rules<'d> {
    fn new(location: &'d LocationDefinition, reading: &[&'d LocationDefinition]) -> Self {
        let secrets = std::iter::once(location)
            .chain(reading.iter().copied())
            .map(|location| &location.secrets)
            .collect();
        Rules { location, secrets }
    }

    /// The value the Agent encrypted that `value` is, masked, if it is one.
    fn encrypted(&self, value: &Value) -> Option<Value> {
        let text = value.as_str()?;
        let prefix = self.encrypted_prefix(text)?;
        Some(Value::String(format!("{prefix}{MASK}")))
    }

    /// Every value the Agent encrypted at or below `value`, found at `path`,
    /// with its masked value.
    fn encrypted_leaves(
        &self,
        value: &Value,
        path: &mut Vec<String>,
        out: &mut Vec<(Vec<String>, Value)>,
    ) {
        if let Some(secret) = self.encrypted(value) {
            out.push((path.clone(), secret));
            return;
        }
        let children: Vec<(String, &Value)> = match value {
            Value::Object(object) => object
                .iter()
                .map(|(key, child)| (key.clone(), child))
                .collect(),
            Value::Array(array) => array
                .iter()
                .enumerate()
                .map(|(index, child)| (index.to_string(), child))
                .collect(),
            _ => return,
        };
        for (key, child) in children {
            path.push(key);
            self.encrypted_leaves(child, path, out);
            path.pop();
        }
    }

    /// Masks every Secret at or below `value`, found at `path` below the Entry.
    fn walk(&self, value: &mut Value, path: &mut Vec<String>, masked: &mut Vec<Vec<String>>) {
        if self.is_variable_name(path) {
            return;
        }
        if let Some(secret) = self.encrypted(value) {
            *value = secret;
            masked.push(path.clone());
            return;
        }
        let env_or_header =
            matches!(path.as_slice(), [field, _, ..] if field == "env" || field == "headers");
        if env_or_header || self.is_secret_field(path) {
            self.mask_all(value, path, masked);
            return;
        }
        let secret = match (path.as_slice(), &*value) {
            ([field], Value::String(text)) if field == "url" => self.url(text).map(Value::String),
            (_, Value::String(text)) if self.is_url_field(path) => {
                self.url(text).map(Value::String)
            }
            ([field], Value::Array(_)) if field == "args" => {
                self.args(value, path, masked);
                return;
            }
            _ => None,
        };
        if let Some(secret) = secret {
            *value = secret;
            masked.push(path.clone());
            return;
        }
        self.children(value, path, masked, Self::walk);
    }

    /// Masks every value at or below `value`, a field the definition marks.
    fn mask_all(&self, value: &mut Value, path: &mut Vec<String>, masked: &mut Vec<Vec<String>>) {
        if self.is_variable_name(path) {
            return;
        }
        if let Some(secret) = self.secret(value) {
            *value = secret;
            masked.push(path.clone());
            return;
        }
        self.children(value, path, masked, Self::mask_all);
    }

    /// Masks every value at or below `value`, a field outside any Entry.
    fn mask_leaves(
        &self,
        value: &mut Value,
        path: &mut Vec<String>,
        masked: &mut Vec<Vec<String>>,
    ) {
        if let Some(secret) = self.secret(value) {
            *value = secret;
            masked.push(path.clone());
            return;
        }
        self.children(value, path, masked, Self::mask_leaves);
    }

    /// Runs `visit` on each value of an object or array.
    fn children(
        &self,
        value: &mut Value,
        path: &mut Vec<String>,
        masked: &mut Vec<Vec<String>>,
        visit: fn(&Self, &mut Value, &mut Vec<String>, &mut Vec<Vec<String>>),
    ) {
        let children: Vec<(String, &mut Value)> = match value {
            Value::Object(object) => object
                .iter_mut()
                .map(|(key, child)| (key.clone(), child))
                .collect(),
            Value::Array(array) => array
                .iter_mut()
                .enumerate()
                .map(|(index, child)| (index.to_string(), child))
                .collect(),
            _ => return,
        };
        for (key, child) in children {
            path.push(key);
            visit(self, child, path, masked);
            path.pop();
        }
    }

    /// An `args` array, where the value of a token-like flag is a Secret,
    /// whether it follows the flag or is joined to it by `=`, and a URL arg
    /// is masked as a URL.
    fn args(&self, value: &mut Value, path: &mut Vec<String>, masked: &mut Vec<Vec<String>>) {
        let Value::Array(args) = value else {
            return;
        };
        let mut flag: Option<String> = None;
        for (index, arg) in args.iter_mut().enumerate() {
            path.push(index.to_string());
            let secret = match arg {
                _ if self.is_variable_name(path) => None,
                Value::String(text) if self.encrypted_prefix(text).is_some() => {
                    flag = None;
                    self.text(text)
                }
                Value::String(text) => match flag.take() {
                    Some(flag) => self.flag_value(&flag, text),
                    None => match text.split_once('=') {
                        Some((name, value)) if is_token_like(name) => self
                            .flag_value(name, value)
                            .map(|value| format!("{name}={value}")),
                        _ => {
                            if is_token_like(text) {
                                flag = Some(text.clone());
                            }
                            self.url(text)
                        }
                    },
                },
                _ => {
                    flag = None;
                    None
                }
            };
            if let Some(secret) = secret {
                *arg = Value::String(secret);
                masked.push(path.clone());
            }
            path.pop();
        }
    }

    /// A Secret's value masked, or `None` when it holds nothing to mask. A
    /// number is masked whole; `true`, `false` and `null` are not Secrets.
    fn secret(&self, value: &Value) -> Option<Value> {
        match value {
            Value::String(text) => self.text(text).map(Value::String),
            Value::Number(_) => Some(Value::String(MASK.to_owned())),
            _ => None,
        }
    }

    /// A Secret's text with its literal text beyond a recognised prefix
    /// masked, or `None` when there is none, such as a value made only of
    /// references.
    fn text(&self, text: &str) -> Option<String> {
        let prefix = match self.encrypted_prefix(text) {
            Some(encrypted) => encrypted,
            None => prefix(text),
        };
        let rest = &text[prefix.len()..];
        let runs = match &self.location.interpolation {
            Some(interpolation) => interpolation.runs(rest),
            None => vec![crate::definitions::blocks::Run {
                text: rest,
                reference: false,
            }],
        };
        if runs.iter().all(|run| run.reference) {
            return None;
        }
        let mut out = prefix.to_owned();
        for run in runs {
            out.push_str(if run.reference { run.text } else { MASK });
        }
        Some(out)
    }

    /// A URL with its userinfo and the value of each parameter of its query
    /// and of a fragment made of parameters masked, or `None` when it has
    /// none of those, or is not a URL.
    fn url(&self, text: &str) -> Option<String> {
        let authority_start = text.find("://")? + "://".len();
        let (rest, fragment) = match text.find('#') {
            Some(hash) if hash >= authority_start => text.split_at(hash),
            _ => (text, ""),
        };
        let (rest, query) = match rest[authority_start..].find('?') {
            Some(question) => rest.split_at(authority_start + question),
            None => (rest, ""),
        };
        let authority_end = rest[authority_start..]
            .find('/')
            .map_or(rest.len(), |slash| authority_start + slash);
        let authority = &rest[authority_start..authority_end];
        let mut masked = false;
        let mut out = rest[..authority_start].to_owned();
        match authority.rfind('@') {
            Some(at) if at > 0 => match self.text(&authority[..at]) {
                Some(userinfo) => {
                    masked = true;
                    out.push_str(&userinfo);
                    out.push_str(&authority[at..]);
                }
                None => out.push_str(authority),
            },
            _ => out.push_str(authority),
        }
        out.push_str(&rest[authority_end..]);
        if let Some(query) = query.strip_prefix('?') {
            out.push('?');
            out.push_str(&self.parameters(query, &mut masked));
        }
        match fragment.strip_prefix('#') {
            // Such as `#access_token=…` after an OAuth redirect.
            Some(fragment) if fragment.contains('=') => {
                out.push('#');
                out.push_str(&self.parameters(fragment, &mut masked));
            }
            _ => out.push_str(fragment),
        }
        masked.then_some(out)
    }

    /// `name=value` parameters joined by `&`, with each value masked.
    fn parameters(&self, text: &str, masked: &mut bool) -> String {
        let parameters: Vec<String> = text
            .split('&')
            .map(|parameter| match parameter.split_once('=') {
                Some((name, value)) => match self.text(value) {
                    Some(value) => {
                        *masked = true;
                        format!("{name}={value}")
                    }
                    None => parameter.to_owned(),
                },
                None => parameter.to_owned(),
            })
            .collect();
        parameters.join("&")
    }

    /// The value of a token-like flag masked. A header flag's value keeps
    /// the header's name, and an environment flag's `NAME=value` keeps the
    /// variable's name; a bare `NAME` passes a variable through and is not
    /// a Secret.
    fn flag_value(&self, flag: &str, value: &str) -> Option<String> {
        if is_env_flag(flag) {
            let (name, value) = value.split_once('=')?;
            return self.text(value).map(|secret| format!("{name}={secret}"));
        }
        let header = flag == "-H" || flag.to_ascii_lowercase().contains("header");
        match value.split_once(':') {
            Some((name, value)) if header && !name.contains(char::is_whitespace) => {
                let trimmed = value.trim_start();
                let space = &value[..value.len() - trimmed.len()];
                self.text(trimmed)
                    .map(|secret| format!("{name}:{space}{secret}"))
            }
            _ => self.text(value),
        }
    }

    /// The prefix of a value an Agent encrypted that `text` starts with, if any.
    fn encrypted_prefix<'t>(&self, text: &'t str) -> Option<&'t str> {
        self.secrets
            .iter()
            .find_map(|secrets| secrets.encrypted_prefix(text))
    }

    fn is_secret_field(&self, path: &[String]) -> bool {
        self.secrets
            .iter()
            .flat_map(|secrets| &secrets.fields)
            .any(|field| matches(field, path))
    }

    fn is_url_field(&self, path: &[String]) -> bool {
        self.secrets
            .iter()
            .flat_map(|secrets| &secrets.urls)
            .any(|field| matches(field, path))
    }

    fn is_variable_name(&self, path: &[String]) -> bool {
        self.location
            .secrets
            .variable_names
            .iter()
            .any(|field| matches(field, path))
    }
}

/// Whether a field's key path, where `*` is any key, is `path`.
fn matches(field: &[String], path: &[String]) -> bool {
    field.len() == path.len()
        && field
            .iter()
            .zip(path)
            .all(|(pattern, key)| pattern == "*" || pattern == key)
}

/// Whether an arg sets an environment variable as `NAME=value`, as
/// `docker run -e` does.
fn is_env_flag(arg: &str) -> bool {
    arg == "-e" || arg == "--env"
}

/// Whether an arg is a flag whose value is likely a Secret, such as
/// `--api-key`, `--token` or `--header`.
fn is_token_like(arg: &str) -> bool {
    if arg == "-H" || is_env_flag(arg) {
        return true;
    }
    let Some(name) = arg.strip_prefix("--").or_else(|| arg.strip_prefix('-')) else {
        return false;
    };
    let name = name.to_ascii_lowercase();
    name.split(['-', '_', '.'])
        .any(|word| TOKEN_WORDS.contains(&word))
        || TOKEN_ENDINGS.iter().any(|ending| name.ends_with(ending))
}

/// The recognised scheme and token-type prefix `text` starts with, either
/// of which may be empty.
fn prefix(text: &str) -> &str {
    let scheme = SCHEMES
        .iter()
        .find(|scheme| {
            text.get(..scheme.len())
                .is_some_and(|start| start.eq_ignore_ascii_case(scheme))
        })
        .map_or(0, |scheme| scheme.len());
    let token = TOKEN_PREFIXES
        .iter()
        .find(|token| text[scheme..].starts_with(*token))
        .map_or(0, |token| token.len());
    &text[..scheme + token]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const SECRET: &str = "not-a-real-secret";

    /// A location of a definition that writes `extra`.
    fn location(extra: &str) -> LocationDefinition {
        let text = format!(
            r#"
id = "user-mcp"
anchor = {{ block = "home", root = "state" }}
format = {{ block = "json", dialect = "strict" }}
root = ["servers"]
kind = "mcp"
scope = "user"
provenance = "file"
store_class = "plain"
on_read_failure = "ignores_file"
{extra}
"#
        );
        toml::from_str(&text).unwrap()
    }

    fn masked(extra: &str, value: Value) -> (Value, Vec<String>) {
        let masked = entry(&location(extra), &value);
        let paths = masked.key_paths.iter().map(|path| path.join(".")).collect();
        (masked.value, paths)
    }

    #[test]
    fn whole_references_in_declared_namespaces_are_shown_verbatim() {
        let (value, paths) = masked(
            r#"interpolation = { block = "braces", namespaces = ["env", "input"] }"#,
            json!({
                "env": {
                    "A": "${env:GITHUB_TOKEN}",
                    "B": "${input:token}",
                    "C": "${secret:token}",
                    "D": format!("Bearer {SECRET}${{env:SUFFIX}}")
                }
            }),
        );

        assert_eq!(
            value,
            json!({ "env": {
                "A": "${env:GITHUB_TOKEN}",
                "B": "${input:token}",
                "C": "••••",
                "D": "Bearer ••••${env:SUFFIX}"
            }})
        );
        assert_eq!(paths, ["env.C", "env.D"]);
    }

    #[test]
    fn without_an_interpolation_syntax_a_reference_is_literal_text() {
        let (value, _) = masked("", json!({ "env": { "A": "${GITHUB_TOKEN}" } }));

        assert_eq!(value, json!({ "env": { "A": "••••" } }));
    }

    #[test]
    fn fields_a_definition_marks_are_masked_and_variable_names_are_not() {
        let (value, paths) = masked(
            r#"
secrets.fields = [["bearer_token"], ["auth", "*"], ["env"]]
secrets.variable_names = [["bearer_token_env_var"], ["env", "TOKEN_NAME"]]
"#,
            json!({
                "bearer_token": SECRET,
                "bearer_token_env_var": "GITHUB_TOKEN",
                "auth": { "client_secret": SECRET, "scopes": ["repo", 42] },
                "env": { "TOKEN_NAME": "GITHUB_TOKEN", "OTHER": SECRET },
                "command": "server"
            }),
        );

        assert_eq!(
            value,
            json!({
                "bearer_token": "••••",
                "bearer_token_env_var": "GITHUB_TOKEN",
                "auth": { "client_secret": "••••", "scopes": ["••••", "••••"] },
                "env": { "TOKEN_NAME": "GITHUB_TOKEN", "OTHER": "••••" },
                "command": "server"
            })
        );
        assert_eq!(
            paths,
            [
                "bearer_token",
                "auth.client_secret",
                "auth.scopes.0",
                "auth.scopes.1",
                "env.OTHER"
            ]
        );
    }

    #[test]
    fn env_and_header_values_below_an_object_are_masked_too() {
        let (value, paths) = masked("", json!({ "env": { "A": { "B": [SECRET] } } }));

        assert_eq!(value, json!({ "env": { "A": { "B": ["••••"] } } }));
        assert_eq!(paths, ["env.A.B.0"]);
    }
}
