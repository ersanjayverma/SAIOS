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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControlOperator {
    Sequential,
    AndAnd,
    OrOr,
    Background,
}

#[derive(Clone, Debug)]
pub struct ParsedPipeline {
    /// Commands connected by pipes in a single statement.
    pub commands: Vec<ParsedCommand>,
}

#[derive(Clone, Debug)]
pub struct ParsedStatement {
    /// The pipeline(s) in this statement.
    pub pipelines: Vec<ParsedPipeline>,
    /// The control operator that governs execution of the next statement.
    pub operator: ControlOperator,
}

/// Splits `line` into statements separated by unquoted control operators.
fn split_statements(line: &str) -> Vec<(&str, ControlOperator)> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut in_single = false;
    let mut in_double = false;
    let mut pending = ControlOperator::Sequential;

    let chars: Vec<(usize, char)> = line.char_indices().collect();
    let mut idx = 0usize;
    while idx < chars.len() {
        let (byte_idx, ch) = chars[idx];
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '&' if !in_single && !in_double && idx + 1 < chars.len() && chars[idx + 1].1 == '&' => {
                let part = line[start..byte_idx].trim();
                if !part.is_empty() {
                    out.push((part, pending));
                }
                start = byte_idx + 2;
                pending = ControlOperator::AndAnd;
                idx += 2;
                continue;
            }
            '&' if !in_single && !in_double => {
                let part = line[start..byte_idx].trim();
                if !part.is_empty() {
                    out.push((part, pending));
                }
                start = byte_idx + 1;
                pending = ControlOperator::Background;
            }
            '|' if !in_single && !in_double && idx + 1 < chars.len() && chars[idx + 1].1 == '|' => {
                let part = line[start..byte_idx].trim();
                if !part.is_empty() {
                    out.push((part, pending));
                }
                start = byte_idx + 2;
                pending = ControlOperator::OrOr;
                idx += 2;
                continue;
            }
            ';' if !in_single && !in_double => {
                let part = line[start..byte_idx].trim();
                if !part.is_empty() {
                    out.push((part, pending));
                }
                start = byte_idx + 1;
                pending = ControlOperator::Sequential;
            }
            _ => {}
        }

        idx += 1;
    }

    let tail = line[start..].trim();
    if !tail.is_empty() {
        out.push((tail, pending));
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

/// Parses a complete input line into one or more parsed statements.
pub fn parse_line(line: &str) -> Vec<ParsedStatement> {
    let mut statements = Vec::new();
    for (statement, operator) in split_statements(line) {
        let mut pipelines = Vec::new();
        let mut commands = Vec::new();
        for part in split_pipeline(statement) {
            if let Some(cmd) = parse_command(part) {
                commands.push(cmd);
            }
        }
        if !commands.is_empty() {
            pipelines.push(ParsedPipeline { commands });
        }
        if !pipelines.is_empty() {
            statements.push(ParsedStatement {
                pipelines,
                operator,
            });
        }
    }
    statements
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_control_operators_and_redirects() {
        let statements = parse_line("echo hello | grep h && echo ok > out.txt");
        assert_eq!(statements.len(), 2);
        assert_eq!(statements[0].operator, ControlOperator::Sequential);
        assert_eq!(statements[1].operator, ControlOperator::AndAnd);
        assert_eq!(statements[0].pipelines.len(), 1);
        assert_eq!(statements[0].pipelines[0].commands.len(), 2);
        assert_eq!(statements[1].pipelines[0].commands[0].redirections.len(), 1);
    }

    #[test]
    fn parses_background_and_or_or() {
        let statements = parse_line("echo first & echo second || echo third");
        assert_eq!(statements.len(), 3);
        assert_eq!(statements[0].operator, ControlOperator::Background);
        assert_eq!(statements[1].operator, ControlOperator::OrOr);
        assert_eq!(statements[2].operator, ControlOperator::Sequential);
    }
}
