use std::{
    collections::{HashMap, HashSet},
    fs::{self, OpenOptions},
    io::{self, BufRead, BufReader, Read, Write},
    path::PathBuf,
};

use anyhow::{Context, Result};
use serde_json::{json, Map, Value};
use spellbook::Dictionary;

const ES_CL_AFF: &str = include_str!(concat!(env!("OUT_DIR"), "/helix-sst-es-CL.aff"));
const ES_CL_DIC: &str = include_str!(concat!(env!("OUT_DIR"), "/helix-sst-es-CL.dic"));
pub const DICTIONARY_LICENSE: &str =
    include_str!(concat!(env!("OUT_DIR"), "/helix-sst-es-CL.LICENSE"));

const SOURCE: &str = "Helix-SST ortografía";
const ADD_WORD_COMMAND: &str = "helix-sst.addWord";

pub fn run_lsp(user_dictionary: PathBuf) -> Result<i32> {
    let mut server = SpellServer::new(user_dictionary)?;
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut input = BufReader::new(stdin.lock());
    let mut output = stdout.lock();

    while let Some(message) = read_message(&mut input)? {
        let method = message.get("method").and_then(Value::as_str).unwrap_or("");
        let id = message.get("id").cloned();

        match method {
            "initialize" => {
                if let Some(id) = id {
                    send_response(
                        &mut output,
                        id,
                        json!({
                            "capabilities": {
                                "positionEncoding": "utf-16",
                                "textDocumentSync": 1,
                                "codeActionProvider": true,
                                "executeCommandProvider": {
                                    "commands": [ADD_WORD_COMMAND]
                                }
                            },
                            "serverInfo": {
                                "name": "helix-sst-spell",
                                "version": "0.1.0"
                            }
                        }),
                    )?;
                }
            }
            "initialized" | "$/cancelRequest" => {}
            "textDocument/didOpen" => {
                if let Some(params) = message.get("params") {
                    let uri = params.pointer("/textDocument/uri").and_then(Value::as_str);
                    let text = params.pointer("/textDocument/text").and_then(Value::as_str);
                    if let (Some(uri), Some(text)) = (uri, text) {
                        server.documents.insert(uri.to_owned(), text.to_owned());
                        server.publish(uri, text, &mut output)?;
                    }
                }
            }
            "textDocument/didChange" => {
                if let Some(params) = message.get("params") {
                    if let Some(uri) = params.pointer("/textDocument/uri").and_then(Value::as_str) {
                        if let Some(text) = params
                            .get("contentChanges")
                            .and_then(Value::as_array)
                            .and_then(|changes| changes.last())
                            .and_then(|change| change.get("text"))
                            .and_then(Value::as_str)
                        {
                            server.documents.insert(uri.to_owned(), text.to_owned());
                            server.publish(uri, text, &mut output)?;
                        }
                    }
                }
            }
            "textDocument/didSave" => {
                if let Some(uri) = message
                    .pointer("/params/textDocument/uri")
                    .and_then(Value::as_str)
                {
                    if let Some(text) = server.documents.get(uri).cloned() {
                        server.publish(uri, &text, &mut output)?;
                    }
                }
            }
            "textDocument/didClose" => {
                if let Some(uri) = message
                    .pointer("/params/textDocument/uri")
                    .and_then(Value::as_str)
                {
                    server.documents.remove(uri);
                    send_notification(
                        &mut output,
                        "textDocument/publishDiagnostics",
                        json!({ "uri": uri, "diagnostics": [] }),
                    )?;
                }
            }
            "textDocument/codeAction" => {
                if let Some(id) = id {
                    let actions = server.code_actions(
                        message.get("params").unwrap_or(&Value::Null),
                    );
                    send_response(&mut output, id, Value::Array(actions))?;
                }
            }
            "workspace/executeCommand" => {
                let mut added = false;
                if message
                    .pointer("/params/command")
                    .and_then(Value::as_str)
                    == Some(ADD_WORD_COMMAND)
                {
                    if let Some(word) = message
                        .pointer("/params/arguments/0")
                        .and_then(Value::as_str)
                    {
                        added = server.add_user_word(word)?;
                    }
                }
                if let Some(id) = id {
                    send_response(&mut output, id, json!(added))?;
                }
                if added {
                    let documents = server
                        .documents
                        .iter()
                        .map(|(uri, text)| (uri.clone(), text.clone()))
                        .collect::<Vec<_>>();
                    for (uri, text) in documents {
                        server.publish(&uri, &text, &mut output)?;
                    }
                }
            }
            "shutdown" => {
                if let Some(id) = id {
                    send_response(&mut output, id, Value::Null)?;
                }
            }
            "exit" => return Ok(0),
            _ => {
                if let Some(id) = id {
                    send_response(&mut output, id, Value::Null)?;
                }
            }
        }
    }

    Ok(0)
}

struct SpellServer {
    dictionary: Dictionary,
    user_dictionary: PathBuf,
    user_words: HashSet<String>,
    documents: HashMap<String, String>,
}

impl SpellServer {
    fn new(user_dictionary: PathBuf) -> Result<Self> {
        let mut dictionary = Dictionary::new(ES_CL_AFF, ES_CL_DIC)
            .map_err(|error| anyhow::anyhow!("diccionario es-CL inválido: {error}"))?;
        let mut user_words = HashSet::new();

        if let Ok(contents) = fs::read_to_string(&user_dictionary) {
            for line in contents.lines() {
                let word = line.trim();
                if word.is_empty() || word.starts_with('#') {
                    continue;
                }
                if dictionary.add(word).is_ok() {
                    user_words.insert(word.to_lowercase());
                }
            }
        }

        Ok(Self {
            dictionary,
            user_dictionary,
            user_words,
            documents: HashMap::new(),
        })
    }

    fn publish(&self, uri: &str, text: &str, output: &mut impl Write) -> Result<()> {
        send_notification(
            output,
            "textDocument/publishDiagnostics",
            json!({
                "uri": uri,
                "diagnostics": self.diagnostics(text)
            }),
        )
    }

    fn diagnostics(&self, text: &str) -> Vec<Value> {
        let mut diagnostics = Vec::new();
        let mut fenced = false;
        let fence = "\x60\x60\x60";

        for (line_number, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with(fence) || trimmed.starts_with("~~~") {
                fenced = !fenced;
                continue;
            }
            if fenced {
                continue;
            }

            let ignored = ignored_spans(line);
            for (start, end, word) in word_ranges(line) {
                if ignored.iter().any(|(left, right)| start >= *left && start < *right) {
                    continue;
                }
                if !self.should_check(&word) || self.is_correct(&word) {
                    continue;
                }

                diagnostics.push(json!({
                    "range": {
                        "start": {
                            "line": line_number,
                            "character": line[..start].encode_utf16().count()
                        },
                        "end": {
                            "line": line_number,
                            "character": line[..end].encode_utf16().count()
                        }
                    },
                    "severity": 2,
                    "source": SOURCE,
                    "message": format!("Posible error ortográfico: «{word}»"),
                    "data": { "word": word }
                }));
            }
        }

        diagnostics
    }

    fn should_check(&self, word: &str) -> bool {
        let letters = word.chars().filter(|ch| ch.is_alphabetic()).count();
        if letters < 2 {
            return false;
        }
        if word.chars().any(|ch| ch.is_ascii_digit() || ch == '_') {
            return false;
        }
        if word.chars().all(|ch| !ch.is_alphabetic() || ch.is_uppercase()) && letters <= 6 {
            return false;
        }
        true
    }

    fn is_correct(&self, word: &str) -> bool {
        self.user_words.contains(&word.to_lowercase())
            || self.dictionary.check(word)
            || self.dictionary.check(&word.to_lowercase())
    }

    fn code_actions(&self, params: &Value) -> Vec<Value> {
        let Some(uri) = params.pointer("/textDocument/uri").and_then(Value::as_str) else {
            return Vec::new();
        };
        let diagnostics = params
            .pointer("/context/diagnostics")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        let mut actions = Vec::new();
        for diagnostic in diagnostics {
            if diagnostic.get("source").and_then(Value::as_str) != Some(SOURCE) {
                continue;
            }
            let Some(word) = diagnostic.pointer("/data/word").and_then(Value::as_str) else {
                continue;
            };
            let Some(range) = diagnostic.get("range").cloned() else {
                continue;
            };

            let mut suggestions = Vec::new();
            self.dictionary.suggest(word, &mut suggestions);
            suggestions.truncate(6);

            for suggestion in suggestions {
                let mut changes = Map::new();
                changes.insert(
                    uri.to_owned(),
                    json!([{
                        "range": range.clone(),
                        "newText": suggestion
                    }]),
                );
                actions.push(json!({
                    "title": format!("Cambiar «{word}» por «{suggestion}»"),
                    "kind": "quickfix",
                    "diagnostics": [diagnostic.clone()],
                    "edit": {
                        "changes": Value::Object(changes)
                    }
                }));
            }
            actions.push(json!({
                "title": format!("Aceptar «{word}» en Helix-SST"),
                "kind": "quickfix",
                "diagnostics": [diagnostic],
                "command": {
                    "title": "Aceptar palabra",
                    "command": ADD_WORD_COMMAND,
                    "arguments": [word]
                }
            }));
        }
        actions
    }

    fn add_user_word(&mut self, word: &str) -> Result<bool> {
        let word = word.trim();
        if word.is_empty()
            || word.contains('\n')
            || word.contains('\r')
            || word.chars().any(char::is_whitespace)
        {
            return Ok(false);
        }

        let normalized = word.to_lowercase();
        if self.user_words.contains(&normalized) || self.dictionary.check(word) {
            return Ok(false);
        }

        if let Some(parent) = self.user_dictionary.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.user_dictionary)
            .with_context(|| format!("no se pudo abrir {}", self.user_dictionary.display()))?;
        writeln!(file, "{word}")?;

        self.dictionary
            .add(word)
            .map_err(|error| anyhow::anyhow!("no se pudo agregar «{word}»: {error}"))?;
        self.user_words.insert(normalized);
        Ok(true)
    }
}

fn word_ranges(line: &str) -> Vec<(usize, usize, String)> {
    let mut ranges = Vec::new();
    let mut start = None;

    for (index, ch) in line.char_indices() {
        let belongs = ch.is_alphabetic() || matches!(ch, '\'' | '’');
        match (start, belongs) {
            (None, true) => start = Some(index),
            (Some(begin), false) => {
                push_word(line, begin, index, &mut ranges);
                start = None;
            }
            _ => {}
        }
    }
    if let Some(begin) = start {
        push_word(line, begin, line.len(), &mut ranges);
    }

    ranges
}

fn push_word(line: &str, mut start: usize, mut end: usize, out: &mut Vec<(usize, usize, String)>) {
    while start < end {
        let Some(ch) = line[start..end].chars().next() else { break };
        if matches!(ch, '\'' | '’') {
            start += ch.len_utf8();
        } else {
            break;
        }
    }
    while start < end {
        let Some(ch) = line[start..end].chars().next_back() else { break };
        if matches!(ch, '\'' | '’') {
            end -= ch.len_utf8();
        } else {
            break;
        }
    }
    if start < end {
        out.push((start, end, line[start..end].to_owned()));
    }
}

fn ignored_spans(line: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let tick = char::from(96);

    let mut cursor = 0usize;
    while let Some(relative) = line[cursor..].find(tick) {
        let open = cursor + relative;
        let after_open = open + tick.len_utf8();
        if let Some(relative_close) = line[after_open..].find(tick) {
            let close = after_open + relative_close + tick.len_utf8();
            spans.push((open, close));
            cursor = close;
        } else {
            spans.push((open, line.len()));
            break;
        }
    }

    for marker in ["https://", "http://", "www."] {
        let mut cursor = 0usize;
        while let Some(found) = line[cursor..].find(marker) {
            let start = cursor + found;
            let end = line[start..]
                .find(char::is_whitespace)
                .map(|offset| start + offset)
                .unwrap_or(line.len());
            spans.push((start, end));
            cursor = end;
        }
    }

    let mut cursor = 0usize;
    while let Some(open) = line[cursor..].find("](") {
        let start = cursor + open + 2;
        if let Some(close) = line[start..].find(')') {
            let end = start + close + 1;
            spans.push((start, end));
            cursor = end;
        } else {
            break;
        }
    }

    spans
}

fn read_message(reader: &mut impl BufRead) -> Result<Option<Value>> {
    let mut content_length = None;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header)? == 0 {
            return Ok(None);
        }
        if header == "\r\n" || header == "\n" {
            break;
        }
        if let Some(value) = header
            .split_once(':')
            .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
            .map(|(_, value)| value.trim())
        {
            content_length = Some(value.parse::<usize>().context("Content-Length inválido")?);
        }
    }

    let length = content_length.context("mensaje LSP sin Content-Length")?;
    let mut payload = vec![0u8; length];
    reader.read_exact(&mut payload)?;
    Ok(Some(serde_json::from_slice(&payload).context("JSON LSP inválido")?))
}

fn send_response(output: &mut impl Write, id: Value, result: Value) -> Result<()> {
    send_json(output, &json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    }))
}

fn send_notification(output: &mut impl Write, method: &str, params: Value) -> Result<()> {
    send_json(output, &json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params
    }))
}

fn send_json(output: &mut impl Write, value: &Value) -> Result<()> {
    let payload = serde_json::to_vec(value)?;
    write!(output, "Content-Length: {}\r\n\r\n", payload.len())?;
    output.write_all(&payload)?;
    output.flush()?;
    Ok(())
}
