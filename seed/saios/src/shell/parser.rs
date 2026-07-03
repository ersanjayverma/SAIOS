//! Simple shell command parser.
//!
//! Splits an input line into statements, pipelines and individual commands,
//! respecting single and double quotes and recognizing basic I/O redirections.

use alloc::string::String;
use alloc::vec::Vec;

/// Direction of an I/O redirection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RedirectKind {
    Read,
    Write,
    Append,
}

#[derive(Clone, Debug)]
pub struct Redirection {
    /// Direction of the redirection.
    pub kind: RedirectKind,
    /// Target file path.
    pub path: String,
}

#[derive(Clone, Debug)]
pub struct ParsedCommand {
    /// Command name or path.
    pub command: String,
    /// Command arguments.
    pub args: Vec<String>,
    /// I/O redirections attached to the command.
    pub redirections: Vec<Redirection>,
}

#[derive(Clone, Debug)]
pub struct ParsedPipeline {
    /// Commands connected by pipes in a single statement.
    pub commands: Vec<ParsedCommand>,
}

/// Splits `line` into statements separated by unquoted semicolons.
fn split_statements(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut in_single = false;
    let mut in_double = false;

    for (idx, ch) in line.char_indices() {
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            ';' if !in_single && !in_double => {
                let part = line[start..idx].trim();
                if !part.is_empty() {
                    out.push(part);
                }
                start = idx + ch.len_utf8();
            }
            // Other characters are part of the statement.
            _ => {}
        }
    }

    let tail = line[start..].trim();
    if !tail.is_empty() {
        out.push(tail);
    }

    out
}

/// Splits `statement` into pipeline stages separated by unquoted pipes.
fn split_pipeline(statement: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut in_single = false;
    let mut in_double = false;

    for (idx, ch) in statement.char_indices() {
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '|' if !in_single && !in_double => {
                let part = statement[start..idx].trim();
                if !part.is_empty() {
                    out.push(part);
                }
                start = idx + ch.len_utf8();
            }
            // Other characters are part of the pipeline stage.
            _ => {}
        }
    }

    let tail = statement[start..].trim();
    if !tail.is_empty() {
        out.push(tail);
    }

    out
}

/// Tokenizes a single pipeline stage into words and redirection operators.
///
/// Quote characters toggle quoting state and are stripped from the resulting
/// tokens.
fn tokenize(part: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut chars = part.chars().peekable();
    let mut in_single = false;
    let mut in_double = false;

    while let Some(ch) = chars.next() {
        match ch {
            '\'' if !in_double => {
                in_single = !in_single;
            }
            '"' if !in_single => {
                in_double = !in_double;
            }
            ' ' | '\t' if !in_single && !in_double => {
                if !current.is_empty() {
                    out.push(core::mem::take(&mut current));
                }
            }
            '>' if !in_single && !in_double => {
                if !current.is_empty() {
                    out.push(core::mem::take(&mut current));
                }
                if chars.peek() == Some(&'>') {
                    let _ = chars.next();
                    out.push(">>".into());
                } else {
                    out.push(">".into());
                }
            }
            '<' if !in_single && !in_double => {
                if !current.is_empty() {
                    out.push(core::mem::take(&mut current));
                }
                out.push("<".into());
            }
            // Append any other character to the current token.
            _ => current.push(ch),
        }
    }

    if !current.is_empty() {
        out.push(current);
    }

    out
}

/// Parses a single pipeline stage into a command, arguments and redirections.
fn parse_command(part: &str) -> Option<ParsedCommand> {
    let tokens = tokenize(part);
    if tokens.is_empty() {
        return None;
    }

    let mut command = None;
    let mut args = Vec::new();
    let mut redirections = Vec::new();

    let mut i = 0usize;
    while i < tokens.len() {
        let token = &tokens[i];
        match token.as_str() {
            ">" => {
                let path = tokens.get(i + 1)?.clone();
                redirections.push(Redirection {
                    kind: RedirectKind::Write,
                    path,
                });
                i += 2;
                continue;
            }
            ">>" => {
                let path = tokens.get(i + 1)?.clone();
                redirections.push(Redirection {
                    kind: RedirectKind::Append,
                    path,
                });
                i += 2;
                continue;
            }
            "<" => {
                let path = tokens.get(i + 1)?.clone();
                redirections.push(Redirection {
                    kind: RedirectKind::Read,
                    path,
                });
                i += 2;
                continue;
            }
            // Regular word: first becomes the command, rest become arguments.
            _ => {}
        }

        if command.is_none() {
            command = Some(token.clone());
        } else {
            args.push(token.clone());
        }

        i += 1;
    }

    Some(ParsedCommand {
        command: command?,
        args,
        redirections,
    })
}

/// Parses a complete input line into one or more parsed pipelines.
pub fn parse_line(line: &str) -> Vec<ParsedPipeline> {
    let mut pipelines = Vec::new();
    for statement in split_statements(line) {
        let mut commands = Vec::new();
        for part in split_pipeline(statement) {
            if let Some(cmd) = parse_command(part) {
                commands.push(cmd);
            }
        }
        if !commands.is_empty() {
            pipelines.push(ParsedPipeline { commands });
        }
    }
    pipelines
}
