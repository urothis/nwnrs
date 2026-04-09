use std::{
    fs::File,
    io::{self, Read, Write},
    path::Path,
};

use nwnrs_resman::prelude::*;
use tracing::instrument;

use crate::{MODEL_RES_TYPE, Model, ModelError, ModelResult};

/// A non-node item that appears inside a geometry or animation body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AsciiBodyItem {
    /// A comment or statement preserved in body order.
    Element(AsciiElement),
    /// A parsed node block.
    Node(AsciiNode),
}

impl AsciiBodyItem {
    /// Returns the item as an [`AsciiElement`] when it is not a node.
    pub fn as_element(&self) -> Option<&AsciiElement> {
        match self {
            Self::Element(element) => Some(element),
            Self::Node(_node) => None,
        }
    }

    /// Returns the item as an [`AsciiNode`] when it is a node.
    pub fn as_node(&self) -> Option<&AsciiNode> {
        match self {
            Self::Element(_element) => None,
            Self::Node(node) => Some(node),
        }
    }
}

/// A comment or statement preserved from the ASCII source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AsciiElement {
    /// A source comment line, including its original indentation.
    Comment(String),
    /// A parsed statement.
    Statement(AsciiStatement),
}

impl AsciiElement {
    /// Returns the element as a parsed statement when applicable.
    pub fn as_statement(&self) -> Option<&AsciiStatement> {
        match self {
            Self::Comment(_comment) => None,
            Self::Statement(statement) => Some(statement),
        }
    }
}

/// Payload shape used by a multiline MDL statement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsciiPayloadKind {
    /// The statement stores an explicit row count on the header line.
    Counted,
    /// The statement uses a trailing `endlist` marker.
    EndList,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// One parsed ASCII MDL statement.
pub struct AsciiStatement {
    /// Statement keyword as authored.
    pub keyword:      String,
    /// Positional arguments that followed the keyword on the same line.
    pub arguments:    Vec<String>,
    /// Multiline payload marker, when present.
    pub payload_kind: Option<AsciiPayloadKind>,
    /// Rows captured for multiline payload statements.
    pub payload_rows: Vec<Vec<String>>,
}

impl AsciiStatement {
    /// Creates a plain single-line statement.
    pub fn new(keyword: impl Into<String>, arguments: Vec<String>) -> Self {
        Self {
            keyword: keyword.into(),
            arguments,
            payload_kind: None,
            payload_rows: Vec::new(),
        }
    }

    fn with_payload(
        keyword: impl Into<String>,
        arguments: Vec<String>,
        payload_kind: AsciiPayloadKind,
        payload_rows: Vec<Vec<String>>,
    ) -> Self {
        Self {
            keyword: keyword.into(),
            arguments,
            payload_kind: Some(payload_kind),
            payload_rows,
        }
    }

    /// Returns `true` when this statement has a multiline payload.
    pub fn has_payload(&self) -> bool {
        self.payload_kind.is_some()
    }

    /// Returns `true` when the keyword matches `other`, case-insensitively.
    pub fn keyword_is(&self, other: &str) -> bool {
        self.keyword.eq_ignore_ascii_case(other)
    }

    /// Returns argument `index` as `&str` when present.
    pub fn argument(&self, index: usize) -> Option<&str> {
        self.arguments.get(index).map(String::as_str)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// One parsed node block.
pub struct AsciiNode {
    /// Node type token from `node <type> <name>`.
    pub node_type: String,
    /// Node name token from `node <type> <name>`.
    pub name:      String,
    /// Ordered entries inside the node body.
    pub entries:   Vec<AsciiElement>,
}

impl AsciiNode {
    /// Returns the first statement with keyword `keyword`.
    pub fn statement(&self, keyword: &str) -> Option<&AsciiStatement> {
        self.entries
            .iter()
            .filter_map(AsciiElement::as_statement)
            .find(|statement| statement.keyword_is(keyword))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// One parsed animation block.
pub struct AsciiAnimation {
    /// Animation name from `newanim <name> <model>`.
    pub name:       String,
    /// Referenced model name from `newanim <name> <model>`.
    pub model_name: String,
    /// Ordered items within the animation body.
    pub body:       Vec<AsciiBodyItem>,
}

impl AsciiAnimation {
    /// Returns the first statement with keyword `keyword` from the non-node
    /// body items.
    pub fn statement(&self, keyword: &str) -> Option<&AsciiStatement> {
        self.body
            .iter()
            .filter_map(AsciiBodyItem::as_element)
            .filter_map(AsciiElement::as_statement)
            .find(|statement| statement.keyword_is(keyword))
    }

    /// Returns the first node named `name`, case-insensitively.
    pub fn node(&self, name: &str) -> Option<&AsciiNode> {
        self.body
            .iter()
            .filter_map(AsciiBodyItem::as_node)
            .find(|node| node.name.eq_ignore_ascii_case(name))
    }

    /// Iterates the parsed nodes in body order.
    pub fn nodes(&self) -> impl Iterator<Item = &AsciiNode> {
        self.body.iter().filter_map(AsciiBodyItem::as_node)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// A syntax-faithful parsed ASCII MDL model.
pub struct AsciiModel {
    /// Elements that appeared before `beginmodelgeom`.
    pub prefix: Vec<AsciiElement>,
    /// Model name used by `beginmodelgeom`.
    pub geometry_name: String,
    /// Ordered items inside the geometry body.
    pub geometry: Vec<AsciiBodyItem>,
    /// Elements between `endmodelgeom` and the first animation or `donemodel`.
    pub between_geometry_and_animations: Vec<AsciiElement>,
    /// Parsed animation blocks in source order.
    pub animations: Vec<AsciiAnimation>,
    /// Elements between adjacent animation blocks, in source order.
    pub between_animations: Vec<Vec<AsciiElement>>,
    /// Elements between the last animation and `donemodel`.
    pub suffix: Vec<AsciiElement>,
    /// Model name used by `donemodel`.
    pub done_model_name: String,
}

impl AsciiModel {
    /// Returns the first statement with keyword `keyword` from the prefix
    /// section.
    pub fn prefix_statement(&self, keyword: &str) -> Option<&AsciiStatement> {
        self.prefix
            .iter()
            .filter_map(AsciiElement::as_statement)
            .find(|statement| statement.keyword_is(keyword))
    }

    /// Returns the first geometry node named `name`, case-insensitively.
    pub fn geometry_node(&self, name: &str) -> Option<&AsciiNode> {
        self.geometry
            .iter()
            .filter_map(AsciiBodyItem::as_node)
            .find(|node| node.name.eq_ignore_ascii_case(name))
    }

    /// Iterates geometry nodes in source order.
    pub fn geometry_nodes(&self) -> impl Iterator<Item = &AsciiNode> {
        self.geometry.iter().filter_map(AsciiBodyItem::as_node)
    }

    /// Returns the first animation named `name`, case-insensitively.
    pub fn animation(&self, name: &str) -> Option<&AsciiAnimation> {
        self.animations
            .iter()
            .find(|animation| animation.name.eq_ignore_ascii_case(name))
    }

    /// Serializes the parsed ASCII model using canonical indentation.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for element in &self.prefix {
            write_element(&mut out, element, 0);
        }
        write_statement_line(
            &mut out,
            0,
            "beginmodelgeom",
            &[self.geometry_name.as_str()],
        );
        for item in &self.geometry {
            write_body_item(&mut out, item, 0);
        }
        write_statement_line(&mut out, 0, "endmodelgeom", &[self.geometry_name.as_str()]);
        for element in &self.between_geometry_and_animations {
            write_element(&mut out, element, 0);
        }
        if let Some(first) = self.animations.first() {
            write_statement_line(&mut out, 0, "newanim", &[&first.name, &first.model_name]);
            for item in &first.body {
                write_body_item(&mut out, item, 0);
            }
            write_statement_line(&mut out, 0, "doneanim", &[&first.name, &first.model_name]);
        }
        for (separator, animation) in self
            .between_animations
            .iter()
            .zip(self.animations.iter().skip(1))
        {
            for element in separator {
                write_element(&mut out, element, 0);
            }
            write_statement_line(
                &mut out,
                0,
                "newanim",
                &[&animation.name, &animation.model_name],
            );
            for item in &animation.body {
                write_body_item(&mut out, item, 0);
            }
            write_statement_line(
                &mut out,
                0,
                "doneanim",
                &[&animation.name, &animation.model_name],
            );
        }
        for element in &self.suffix {
            write_element(&mut out, element, 0);
        }
        write_statement_line(&mut out, 0, "donemodel", &[self.done_model_name.as_str()]);
        out
    }
}

impl Model {
    /// Parses the raw payload as an ASCII MDL model using Latin-1 byte mapping.
    pub fn parse_ascii(&self) -> ModelResult<AsciiModel> {
        parse_ascii_model_bytes(self.bytes())
    }
}

/// Parses an ASCII MDL model from raw text.
pub fn parse_ascii_model(text: &str) -> ModelResult<AsciiModel> {
    Parser::new(text).parse_model()
}

fn parse_ascii_model_bytes(bytes: &[u8]) -> ModelResult<AsciiModel> {
    let text: String = bytes.iter().map(|byte| char::from(*byte)).collect();
    parse_ascii_model(&text)
}

/// Reads an ASCII MDL model from `reader`.
#[instrument(level = "debug", skip_all, err)]
pub fn read_ascii_model<R: Read>(reader: &mut R) -> ModelResult<AsciiModel> {
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes)?;
    parse_ascii_model_bytes(&bytes)
}

/// Reads an ASCII MDL model from disk.
#[instrument(level = "debug", skip_all, err, fields(path = %path.as_ref().display()))]
pub fn read_ascii_model_from_file(path: impl AsRef<Path>) -> ModelResult<AsciiModel> {
    let mut file = File::open(path.as_ref())?;
    read_ascii_model(&mut file)
}

/// Reads an ASCII MDL model from a [`Res`].
#[instrument(level = "debug", skip_all, err, fields(resref = %res.resref(), use_cache))]
pub fn read_ascii_model_from_res(res: &Res, use_cache: bool) -> ModelResult<AsciiModel> {
    if res.resref().res_type() != MODEL_RES_TYPE {
        return Err(ModelError::msg(format!(
            "expected mdl resource, got {}",
            res.resref()
        )));
    }

    let bytes = res.read_all(use_cache)?;
    parse_ascii_model_bytes(&bytes)
}

/// Writes a parsed ASCII MDL model using canonical indentation.
#[instrument(level = "debug", skip_all, err, fields(geometry_name = %model.geometry_name))]
pub fn write_ascii_model<W: Write>(writer: &mut W, model: &AsciiModel) -> io::Result<()> {
    writer.write_all(model.to_text().as_bytes())
}

struct Parser<'a> {
    lines: Vec<&'a str>,
    index: usize,
}

impl<'a> Parser<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            lines: text.lines().collect(),
            index: 0,
        }
    }

    fn parse_model(mut self) -> ModelResult<AsciiModel> {
        let mut prefix = Vec::new();
        while let Some(line) = self.peek_meaningful() {
            if keyword_of(line)
                .is_some_and(|keyword| keyword.eq_ignore_ascii_case("beginmodelgeom"))
            {
                break;
            }
            prefix.push(self.parse_element()?);
        }

        let begin_geom = self.parse_statement()?;
        if !begin_geom.keyword_is("beginmodelgeom") {
            return Err(ModelError::msg("ASCII MDL is missing beginmodelgeom"));
        }
        let geometry_name = begin_geom
            .argument(0)
            .ok_or_else(|| ModelError::msg("beginmodelgeom requires a model name"))?
            .to_string();

        let mut geometry = Vec::new();
        loop {
            let Some(line) = self.peek_meaningful() else {
                return Err(ModelError::msg("ASCII MDL ended before endmodelgeom"));
            };
            let keyword =
                keyword_of(line).ok_or_else(|| ModelError::msg("invalid geometry line"))?;
            if keyword.eq_ignore_ascii_case("endmodelgeom") {
                let end_geom = self.parse_statement()?;
                let end_name = end_geom
                    .argument(0)
                    .ok_or_else(|| ModelError::msg("endmodelgeom requires a model name"))?;
                if !end_name.eq_ignore_ascii_case(&geometry_name) {
                    return Err(ModelError::msg(format!(
                        "endmodelgeom name mismatch: expected {geometry_name}, got {end_name}"
                    )));
                }
                break;
            }
            geometry.push(self.parse_body_item()?);
        }

        let mut between_geometry_and_animations = Vec::new();
        while let Some(line) = self.peek_meaningful() {
            let keyword = keyword_of(line)
                .ok_or_else(|| ModelError::msg("invalid top-level line after geometry"))?;
            if keyword.eq_ignore_ascii_case("newanim") || keyword.eq_ignore_ascii_case("donemodel")
            {
                break;
            }
            between_geometry_and_animations.push(self.parse_element()?);
        }

        let mut animations = Vec::new();
        let mut between_animations = Vec::new();
        let mut suffix = Vec::new();
        if self.peek_meaningful().is_some_and(|line| {
            keyword_of(line).is_some_and(|keyword| keyword.eq_ignore_ascii_case("newanim"))
        }) {
            animations.push(self.parse_animation()?);
            loop {
                let mut separator = Vec::new();
                while let Some(line) = self.peek_meaningful() {
                    if keyword_of(line).is_some_and(|keyword| {
                        keyword.eq_ignore_ascii_case("newanim")
                            || keyword.eq_ignore_ascii_case("donemodel")
                    }) {
                        break;
                    }
                    separator.push(self.parse_element()?);
                }

                if self.peek_meaningful().is_some_and(|line| {
                    keyword_of(line).is_some_and(|keyword| keyword.eq_ignore_ascii_case("newanim"))
                }) {
                    between_animations.push(separator);
                    animations.push(self.parse_animation()?);
                    continue;
                }

                suffix.extend(separator);
                break;
            }
        }
        while let Some(line) = self.peek_meaningful() {
            if keyword_of(line).is_some_and(|keyword| keyword.eq_ignore_ascii_case("donemodel")) {
                break;
            }
            suffix.push(self.parse_element()?);
        }

        let done_model = self.parse_statement()?;
        if !done_model.keyword_is("donemodel") {
            return Err(ModelError::msg("ASCII MDL is missing donemodel"));
        }
        let done_model_name = done_model
            .argument(0)
            .ok_or_else(|| ModelError::msg("donemodel requires a model name"))?
            .to_string();
        if !done_model_name.eq_ignore_ascii_case(&geometry_name) {
            return Err(ModelError::msg(format!(
                "donemodel name mismatch: expected {geometry_name}, got {done_model_name}"
            )));
        }

        Ok(AsciiModel {
            prefix,
            geometry_name,
            geometry,
            between_geometry_and_animations,
            animations,
            between_animations,
            suffix,
            done_model_name,
        })
    }

    fn parse_animation(&mut self) -> ModelResult<AsciiAnimation> {
        let new_anim = self.parse_statement()?;
        if !new_anim.keyword_is("newanim") {
            return Err(ModelError::msg("animation must start with newanim"));
        }
        let name = new_anim
            .argument(0)
            .ok_or_else(|| ModelError::msg("newanim requires an animation name"))?
            .to_string();
        let model_name = new_anim
            .argument(1)
            .ok_or_else(|| ModelError::msg("newanim requires a model name"))?
            .to_string();

        let mut body = Vec::new();
        loop {
            let Some(line) = self.peek_meaningful() else {
                return Err(ModelError::msg(format!(
                    "animation {name} ended before doneanim"
                )));
            };
            let keyword = keyword_of(line)
                .ok_or_else(|| ModelError::msg(format!("invalid line in animation {name}")))?;
            if keyword.eq_ignore_ascii_case("doneanim") {
                let done_anim = self.parse_statement()?;
                let done_name = done_anim
                    .argument(0)
                    .ok_or_else(|| ModelError::msg("doneanim requires an animation name"))?;
                let done_model = done_anim
                    .argument(1)
                    .ok_or_else(|| ModelError::msg("doneanim requires a model name"))?;
                if !done_name.eq_ignore_ascii_case(&name)
                    || !done_model.eq_ignore_ascii_case(&model_name)
                {
                    return Err(ModelError::msg(format!(
                        "doneanim mismatch: expected {name} {model_name}, got {done_name} \
                         {done_model}"
                    )));
                }
                break;
            }
            body.push(self.parse_body_item()?);
        }

        Ok(AsciiAnimation {
            name,
            model_name,
            body,
        })
    }

    fn parse_body_item(&mut self) -> ModelResult<AsciiBodyItem> {
        let line = self
            .peek_meaningful()
            .ok_or_else(|| ModelError::msg("unexpected end of body"))?;
        if keyword_of(line).is_some_and(|keyword| keyword.eq_ignore_ascii_case("node")) {
            Ok(AsciiBodyItem::Node(self.parse_node()?))
        } else {
            Ok(AsciiBodyItem::Element(self.parse_element()?))
        }
    }

    fn parse_node(&mut self) -> ModelResult<AsciiNode> {
        let header = self.parse_statement()?;
        if !header.keyword_is("node") {
            return Err(ModelError::msg("node block must start with node"));
        }
        let node_type = header
            .argument(0)
            .ok_or_else(|| ModelError::msg("node header requires a node type"))?
            .to_string();
        let name = header
            .argument(1)
            .ok_or_else(|| ModelError::msg("node header requires a node name"))?
            .to_string();

        let mut entries = Vec::new();
        loop {
            let Some(line) = self.peek_meaningful() else {
                return Err(ModelError::msg(format!("node {name} ended before endnode")));
            };
            if keyword_of(line).is_some_and(|keyword| keyword.eq_ignore_ascii_case("endnode")) {
                let endnode = self.parse_statement()?;
                if !endnode.keyword_is("endnode") {
                    return Err(ModelError::msg("node terminator must be endnode"));
                }
                break;
            }
            entries.push(self.parse_element()?);
        }

        Ok(AsciiNode {
            node_type,
            name,
            entries,
        })
    }

    fn parse_element(&mut self) -> ModelResult<AsciiElement> {
        self.skip_blank_lines();
        let line = self
            .peek()
            .ok_or_else(|| ModelError::msg("unexpected end of input"))?;
        if line.trim_start().starts_with('#') {
            let comment = self
                .next()
                .ok_or_else(|| ModelError::msg("unexpected end of comment"))?;
            return Ok(AsciiElement::Comment(comment.trim_end().to_string()));
        }
        Ok(AsciiElement::Statement(self.parse_statement()?))
    }

    fn parse_statement(&mut self) -> ModelResult<AsciiStatement> {
        self.skip_blank_lines();
        let line = self
            .next()
            .ok_or_else(|| ModelError::msg("unexpected end of statement"))?;
        let indent = indentation_of(line);
        let trimmed = line.trim();
        let parts = split_tokens(trimmed);
        let Some((keyword, raw_arguments)) = parts.split_first() else {
            return Err(ModelError::msg("empty statement"));
        };

        let keyword_lower = keyword.to_ascii_lowercase();
        if statement_supports_payload(&keyword_lower) {
            if let Some(count) = raw_arguments
                .first()
                .and_then(|arg| arg.parse::<usize>().ok())
            {
                let payload_rows = self.read_counted_payload_rows(count)?;
                return Ok(AsciiStatement::with_payload(
                    keyword.clone(),
                    raw_arguments.get(1..).unwrap_or(&[]).to_vec(),
                    AsciiPayloadKind::Counted,
                    payload_rows,
                ));
            }

            if self
                .peek_meaningful()
                .is_some_and(|next| indentation_of(next) > indent)
            {
                let payload_rows = self.read_endlist_payload_rows()?;
                return Ok(AsciiStatement::with_payload(
                    keyword.clone(),
                    raw_arguments.to_vec(),
                    AsciiPayloadKind::EndList,
                    payload_rows,
                ));
            }
        }

        Ok(AsciiStatement::new(keyword.clone(), raw_arguments.to_vec()))
    }

    fn read_counted_payload_rows(&mut self, count: usize) -> ModelResult<Vec<Vec<String>>> {
        let mut rows = Vec::with_capacity(count);
        while rows.len() < count {
            self.skip_blank_lines();
            let line = self
                .next()
                .ok_or_else(|| ModelError::msg("payload ended before expected row count"))?;
            if line.trim_start().starts_with('#') {
                return Err(ModelError::msg(
                    "comments inside counted payload blocks are not supported",
                ));
            }
            rows.push(split_tokens(line.trim()));
        }
        Ok(rows)
    }

    fn read_endlist_payload_rows(&mut self) -> ModelResult<Vec<Vec<String>>> {
        let mut rows = Vec::new();
        loop {
            self.skip_blank_lines();
            let line = self
                .next()
                .ok_or_else(|| ModelError::msg("payload ended before endlist"))?;
            let trimmed = line.trim();
            if trimmed.eq_ignore_ascii_case("endlist") {
                return Ok(rows);
            }
            if trimmed.starts_with('#') {
                return Err(ModelError::msg(
                    "comments inside endlist payload blocks are not supported",
                ));
            }
            rows.push(split_tokens(trimmed));
        }
    }

    fn skip_blank_lines(&mut self) {
        while self.peek().is_some_and(|line| line.trim().is_empty()) {
            self.index += 1;
        }
    }

    fn peek(&self) -> Option<&'a str> {
        self.lines.get(self.index).copied()
    }

    fn peek_meaningful(&mut self) -> Option<&'a str> {
        self.skip_blank_lines();
        self.peek()
    }

    fn next(&mut self) -> Option<&'a str> {
        let line = self.peek()?;
        self.index += 1;
        Some(line)
    }
}

fn split_tokens(line: &str) -> Vec<String> {
    line.split_whitespace().map(ToOwned::to_owned).collect()
}

fn indentation_of(line: &str) -> usize {
    line.chars().take_while(|char| char.is_whitespace()).count()
}

fn keyword_of(line: &str) -> Option<&str> {
    line.split_whitespace().next()
}

fn statement_supports_payload(keyword: &str) -> bool {
    keyword.ends_with("key")
        || keyword == "multimaterial"
        || keyword == "texturenames"
        || keyword.strip_prefix("tverts").is_some_and(|suffix| {
            suffix.is_empty() || suffix.chars().all(|char| char.is_ascii_digit())
        })
        || matches!(
            keyword,
            "animtverts"
                | "animverts"
                | "colors"
                | "constraints"
                | "faces"
                | "flarecolorshifts"
                | "flarepositions"
                | "flaresizes"
                | "lensflares"
                | "normals"
                | "tangents"
                | "verts"
                | "weights"
        )
}

fn write_body_item(out: &mut String, item: &AsciiBodyItem, indent: usize) {
    match item {
        AsciiBodyItem::Element(element) => write_element(out, element, indent),
        AsciiBodyItem::Node(node) => write_node(out, node, indent),
    }
}

fn write_node(out: &mut String, node: &AsciiNode, indent: usize) {
    write_statement_line(out, indent, "node", &[&node.node_type, &node.name]);
    for entry in &node.entries {
        write_element(out, entry, indent + 2);
    }
    write_statement_line(out, indent, "endnode", &[]);
}

fn write_element(out: &mut String, element: &AsciiElement, indent: usize) {
    match element {
        AsciiElement::Comment(comment) => {
            if indent == 0 {
                out.push_str(comment);
            } else {
                out.push_str(&" ".repeat(indent));
                out.push_str(comment.trim_start());
            }
            out.push('\n');
        }
        AsciiElement::Statement(statement) => write_statement(out, statement, indent),
    }
}

fn write_statement(out: &mut String, statement: &AsciiStatement, indent: usize) {
    match statement.payload_kind {
        None => {
            let arguments: Vec<&str> = statement.arguments.iter().map(String::as_str).collect();
            write_statement_line(out, indent, &statement.keyword, &arguments);
        }
        Some(AsciiPayloadKind::Counted) => {
            let mut arguments = Vec::with_capacity(statement.arguments.len() + 1);
            arguments.push(statement.payload_rows.len().to_string());
            arguments.extend(statement.arguments.iter().cloned());
            let arguments: Vec<&str> = arguments.iter().map(String::as_str).collect();
            write_statement_line(out, indent, &statement.keyword, &arguments);
            for row in &statement.payload_rows {
                write_row_line(out, indent + 2, row);
            }
        }
        Some(AsciiPayloadKind::EndList) => {
            let arguments: Vec<&str> = statement.arguments.iter().map(String::as_str).collect();
            write_statement_line(out, indent, &statement.keyword, &arguments);
            for row in &statement.payload_rows {
                write_row_line(out, indent + 2, row);
            }
            write_statement_line(out, indent, "endlist", &[]);
        }
    }
}

fn write_statement_line(out: &mut String, indent: usize, keyword: &str, arguments: &[&str]) {
    out.push_str(&" ".repeat(indent));
    out.push_str(keyword);
    for argument in arguments {
        out.push(' ');
        out.push_str(argument);
    }
    out.push('\n');
}

fn write_row_line(out: &mut String, indent: usize, row: &[String]) {
    out.push_str(&" ".repeat(indent));
    let mut parts = row.iter();
    if let Some(first) = parts.next() {
        out.push_str(first);
    }
    for value in parts {
        out.push(' ');
        out.push_str(value);
    }
    out.push('\n');
}

#[allow(clippy::panic)]
#[cfg(test)]
mod tests {
    use std::{fs, io::Cursor, path::PathBuf};

    use crate::{
        AsciiElement, AsciiPayloadKind, Model, parse_ascii_model, read_ascii_model_from_file,
        write_ascii_model,
    };

    fn fixture_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/testing/test.mdl")
    }

    #[test]
    fn fixture_parses_geometry_and_animation_structure() {
        let ascii = read_ascii_model_from_file(fixture_path()).unwrap_or_else(|error| {
            panic!("read ascii mdl fixture: {error}");
        });

        assert_eq!(ascii.geometry_name, "a_ba_casts");
        assert_eq!(
            ascii
                .prefix_statement("newmodel")
                .and_then(|statement| statement.argument(0)),
            Some("a_ba_casts")
        );
        assert!(ascii.geometry_nodes().count() > 10);
        assert_eq!(ascii.animations.len(), 19);

        let conjure = ascii.animation("conjure1").unwrap_or_else(|| {
            panic!("missing conjure1 animation");
        });
        assert_eq!(
            conjure
                .statement("animroot")
                .and_then(|statement| statement.argument(0)),
            Some("rootdummy")
        );

        let rootdummy = conjure.node("rootdummy").unwrap_or_else(|| {
            panic!("missing rootdummy animation node");
        });
        let positionkey = rootdummy.statement("positionkey").unwrap_or_else(|| {
            panic!("missing rootdummy positionkey");
        });
        assert_eq!(positionkey.payload_kind, Some(AsciiPayloadKind::Counted));
        assert_eq!(positionkey.payload_rows.len(), 5);

        let eventful = ascii.animation("castout").unwrap_or_else(|| {
            panic!("missing castout animation");
        });
        assert_eq!(
            eventful
                .statement("event")
                .and_then(|statement| statement.argument(1)),
            Some("cast")
        );
    }

    #[test]
    fn parser_supports_endlist_key_blocks() {
        let sample = "\
newmodel demo
setsupermodel demo null
classification character
setanimationscale 1
beginmodelgeom demo
node dummy demo
  parent null
endnode
endmodelgeom demo
newanim idle demo
node dummy rootdummy
  parent demo
  positionkey
    0.0 0.0 0.0 1.0
    1.0 0.0 0.0 1.0
  endlist
endnode
doneanim idle demo
donemodel demo
";

        let model = parse_ascii_model(sample).unwrap_or_else(|error| {
            panic!("parse endlist sample: {error}");
        });
        let node = model
            .animation("idle")
            .and_then(|animation| animation.node("rootdummy"))
            .unwrap_or_else(|| panic!("missing idle/rootdummy"));
        let positionkey = node.statement("positionkey").unwrap_or_else(|| {
            panic!("missing endlist positionkey");
        });
        assert_eq!(positionkey.payload_kind, Some(AsciiPayloadKind::EndList));
        assert_eq!(positionkey.payload_rows.len(), 2);
    }

    #[test]
    fn canonical_write_roundtrips_through_parse() {
        let source = fs::read(fixture_path()).unwrap_or_else(|error| {
            panic!("read mdl fixture bytes: {error}");
        });
        let parsed = Model::new(source)
            .parse_ascii()
            .unwrap_or_else(|error| panic!("parse fixture: {error}"));

        let mut encoded = Vec::new();
        if let Err(error) = write_ascii_model(&mut encoded, &parsed) {
            panic!("write ascii model: {error}");
        }

        let reparsed =
            parse_ascii_model(&String::from_utf8_lossy(&encoded)).unwrap_or_else(|error| {
                panic!("reparse canonical text: {error}");
            });

        assert_eq!(reparsed, parsed);
    }

    #[test]
    fn comments_are_preserved_in_node_entries() {
        let sample = "\
newmodel demo
setsupermodel demo null
classification character
setanimationscale 1
beginmodelgeom demo
node dummy demo
  #part-number 0
  parent null
endnode
endmodelgeom demo
donemodel demo
";

        let model = parse_ascii_model(sample).unwrap_or_else(|error| {
            panic!("parse comment sample: {error}");
        });
        let node = model
            .geometry_node("demo")
            .unwrap_or_else(|| panic!("missing geometry node"));
        assert!(matches!(
            node.entries.first(),
            Some(AsciiElement::Comment(comment)) if comment.contains("#part-number 0")
        ));

        let mut encoded = Vec::new();
        if let Err(error) = write_ascii_model(&mut Cursor::new(&mut encoded), &model) {
            panic!("write comment sample: {error}");
        }
        let written = String::from_utf8_lossy(&encoded);
        assert!(written.contains("#part-number 0"));
    }
}
