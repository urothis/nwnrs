pub(crate) mod text;

use std::{
    fs::File,
    io::{Read, Write},
    path::Path,
};

use nwnrs_types::resman::prelude::*;
use tracing::instrument;

use self::text::parse_legacy_f32;
use crate::mdl::{
    MODEL_RES_TYPE, Model, ModelClassification, ModelError, ModelResult, ScalarKey,
    SemanticAnimation, SemanticAnimationNode, SemanticDangly, SemanticEmitter,
    SemanticEmitterController, SemanticEmitterProperty, SemanticFace, SemanticHeader,
    SemanticLight, SemanticMaterial, SemanticMesh, SemanticModel, SemanticNode,
    SemanticPropertyValue, SemanticReference, SemanticSkinWeight, SemanticTextureBinding, Vec3Key,
    Vec4Key,
};

const COMPILED_SOURCE_BEGIN: &str = "# nwnrs-compiled-source begin";
const COMPILED_SOURCE_END: &str = "# nwnrs-compiled-source end";
const COMPILED_SOURCE_HEX_PREFIX: &str = "# nwnrs-compiled-source-hex ";
const HEX_CHUNK_LEN: usize = 120;

/// A non-node item that appears inside a geometry or animation body.
///
/// # Examples
///
/// ```
/// use nwnrs_types::mdl::{AsciiBodyItem, AsciiElement};
/// let item = AsciiBodyItem::Element(AsciiElement::Comment("# note".into()));
/// assert!(matches!(item, AsciiBodyItem::Element(_)));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AsciiBodyItem {
    /// A comment or statement preserved in body order.
    Element(AsciiElement),
    /// A parsed node block.
    Node(AsciiNode),
}

impl AsciiBodyItem {
    /// Returns the item as an [`AsciiElement`] when it is not a node.
    ///
    /// # Examples
    ///
    /// ```
    /// use nwnrs_types::mdl::{AsciiBodyItem, AsciiElement};
    /// let item = AsciiBodyItem::Element(AsciiElement::Comment("# note".into()));
    /// assert!(item.as_element().is_some());
    /// ```
    #[must_use]
    pub fn as_element(&self) -> Option<&AsciiElement> {
        match self {
            Self::Element(element) => Some(element),
            Self::Node(_node) => None,
        }
    }

    /// Returns the item as an [`AsciiNode`] when it is a node.
    ///
    /// # Examples
    ///
    /// ```
    /// use nwnrs_types::mdl::{AsciiBodyItem, AsciiNode};
    /// let item = AsciiBodyItem::Node(AsciiNode {
    ///     node_type: "dummy".into(), name: "root".into(), entries: vec![],
    /// });
    /// assert_eq!(item.as_node().map(|node| node.name.as_str()), Some("root"));
    /// ```
    #[must_use]
    pub fn as_node(&self) -> Option<&AsciiNode> {
        match self {
            Self::Element(_element) => None,
            Self::Node(node) => Some(node),
        }
    }
}

/// A comment or statement preserved from the ASCII source.
///
/// # Examples
///
/// ```
/// use nwnrs_types::mdl::AsciiElement;
/// let element = AsciiElement::Comment("# exported by NWMax".into());
/// assert!(matches!(element, AsciiElement::Comment(_)));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AsciiElement {
    /// A source comment line, including its original indentation.
    Comment(String),
    /// A parsed statement.
    Statement(AsciiStatement),
}

impl AsciiElement {
    /// Returns the element as a parsed statement when applicable.
    ///
    /// # Examples
    ///
    /// ```
    /// use nwnrs_types::mdl::{AsciiElement, AsciiStatement};
    /// let element = AsciiElement::Statement(AsciiStatement::new("parent", vec!["null".into()]));
    /// assert_eq!(element.as_statement().and_then(|value| value.argument(0)), Some("null"));
    /// ```
    #[must_use]
    pub fn as_statement(&self) -> Option<&AsciiStatement> {
        match self {
            Self::Comment(_comment) => None,
            Self::Statement(statement) => Some(statement),
        }
    }
}

/// Payload shape used by a multiline MDL statement.
///
/// # Examples
///
/// ```
/// use nwnrs_types::mdl::AsciiPayloadKind;
/// let kind = AsciiPayloadKind::EndList;
/// assert_eq!(kind, AsciiPayloadKind::EndList);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsciiPayloadKind {
    /// The statement stores an explicit row count on the header line.
    Counted,
    /// The statement uses a trailing `endlist` marker.
    EndList,
    /// Continuation rows are identified by deeper indentation.
    Indented,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// One parsed ASCII MDL statement.
///
/// # Examples
///
/// ```
/// use nwnrs_types::mdl::AsciiStatement;
/// let statement = AsciiStatement::new("parent", vec!["root".into()]);
/// assert_eq!(statement.keyword, "parent");
/// ```
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
    ///
    /// # Examples
    ///
    /// ```
    /// let statement = nwnrs_types::mdl::AsciiStatement::new(
    ///     "parent",
    ///     vec!["null".to_string()],
    /// );
    /// assert_eq!(statement.argument(0), Some("null"));
    /// assert!(!statement.has_payload());
    /// ```
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
    ///
    /// # Examples
    ///
    /// ```
    /// let statement = nwnrs_types::mdl::AsciiStatement::new("parent", vec!["null".into()]);
    /// assert!(!statement.has_payload());
    /// ```
    #[must_use]
    pub fn has_payload(&self) -> bool {
        self.payload_kind.is_some()
    }

    /// Returns `true` when the keyword matches `other`, case-insensitively.
    ///
    /// # Examples
    ///
    /// ```
    /// let statement = nwnrs_types::mdl::AsciiStatement::new("Parent", vec!["null".into()]);
    /// assert!(statement.keyword_is("parent"));
    /// ```
    #[must_use]
    pub fn keyword_is(&self, other: &str) -> bool {
        self.keyword.eq_ignore_ascii_case(other)
    }

    /// Returns argument `index` as `&str` when present.
    ///
    /// # Examples
    ///
    /// ```
    /// let statement = nwnrs_types::mdl::AsciiStatement::new("parent", vec!["root".into()]);
    /// assert_eq!(statement.argument(0), Some("root"));
    /// ```
    pub fn argument(&self, index: usize) -> Option<&str> {
        self.arguments.get(index).map(String::as_str)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// One parsed node block.
///
/// # Examples
///
/// ```
/// use nwnrs_types::mdl::AsciiNode;
/// let node = AsciiNode { node_type: "dummy".into(), name: "root".into(), entries: vec![] };
/// assert_eq!(node.name, "root");
/// ```
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
    ///
    /// # Examples
    ///
    /// ```
    /// use nwnrs_types::mdl::{AsciiElement, AsciiNode, AsciiStatement};
    /// let node = AsciiNode {
    ///     node_type: "dummy".into(), name: "root".into(),
    ///     entries: vec![AsciiElement::Statement(AsciiStatement::new("parent", vec!["null".into()]))],
    /// };
    /// assert!(node.statement("parent").is_some());
    /// ```
    pub fn statement(&self, keyword: &str) -> Option<&AsciiStatement> {
        self.entries
            .iter()
            .filter_map(AsciiElement::as_statement)
            .find(|statement| statement.keyword_is(keyword))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// One parsed animation block.
///
/// # Examples
///
/// ```
/// use nwnrs_types::mdl::AsciiAnimation;
/// let animation = AsciiAnimation { name: "idle".into(), model_name: "demo".into(), body: vec![] };
/// assert_eq!(animation.name, "idle");
/// ```
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
    ///
    /// # Examples
    ///
    /// ```
    /// # use nwnrs_types::mdl::{AsciiAnimation, AsciiBodyItem, AsciiElement, AsciiStatement};
    /// let animation = AsciiAnimation { name: "idle".into(), model_name: "demo".into(), body: vec![
    ///     AsciiBodyItem::Element(AsciiElement::Statement(AsciiStatement::new("length", vec!["1".into()])))
    /// ] };
    /// assert!(animation.statement("length").is_some());
    /// ```
    pub fn statement(&self, keyword: &str) -> Option<&AsciiStatement> {
        self.body
            .iter()
            .filter_map(AsciiBodyItem::as_element)
            .filter_map(AsciiElement::as_statement)
            .find(|statement| statement.keyword_is(keyword))
    }

    /// Returns the first node named `name`, case-insensitively.
    ///
    /// # Examples
    ///
    /// ```
    /// # use nwnrs_types::mdl::{AsciiAnimation, AsciiBodyItem, AsciiNode};
    /// let animation = AsciiAnimation { name: "idle".into(), model_name: "demo".into(), body: vec![
    ///     AsciiBodyItem::Node(AsciiNode { node_type: "dummy".into(), name: "root".into(), entries: vec![] })
    /// ] };
    /// assert!(animation.node("ROOT").is_some());
    /// ```
    pub fn node(&self, name: &str) -> Option<&AsciiNode> {
        self.body
            .iter()
            .filter_map(AsciiBodyItem::as_node)
            .find(|node| node.name.eq_ignore_ascii_case(name))
    }

    /// Iterates the parsed nodes in body order.
    ///
    /// # Examples
    ///
    /// ```
    /// # use nwnrs_types::mdl::{AsciiAnimation, AsciiBodyItem, AsciiNode};
    /// let animation = AsciiAnimation { name: "idle".into(), model_name: "demo".into(), body: vec![
    ///     AsciiBodyItem::Node(AsciiNode { node_type: "dummy".into(), name: "root".into(), entries: vec![] })
    /// ] };
    /// assert_eq!(animation.nodes().count(), 1);
    /// ```
    pub fn nodes(&self) -> impl Iterator<Item = &AsciiNode> {
        self.body.iter().filter_map(AsciiBodyItem::as_node)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// A syntax-faithful parsed ASCII MDL model.
///
/// # Examples
///
/// ```
/// let model = nwnrs_types::mdl::parse_ascii_model("beginmodelgeom demo\nendmodelgeom demo\ndonemodel demo\n")?;
/// assert_eq!(model.geometry_name, "demo");
/// # Ok::<(), nwnrs_types::mdl::ModelError>(())
/// ```
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
    ///
    /// # Examples
    ///
    /// ```
    /// let model = nwnrs_types::mdl::parse_ascii_model("newmodel demo\nbeginmodelgeom demo\nendmodelgeom demo\ndonemodel demo\n")?;
    /// assert!(model.prefix_statement("newmodel").is_some());
    /// # Ok::<(), nwnrs_types::mdl::ModelError>(())
    /// ```
    pub fn prefix_statement(&self, keyword: &str) -> Option<&AsciiStatement> {
        self.prefix
            .iter()
            .filter_map(AsciiElement::as_statement)
            .find(|statement| statement.keyword_is(keyword))
    }

    /// Returns the first geometry node named `name`, case-insensitively.
    ///
    /// # Examples
    ///
    /// ```
    /// let model = nwnrs_types::mdl::parse_ascii_model("beginmodelgeom demo\nnode dummy root\nendnode\nendmodelgeom demo\ndonemodel demo\n")?;
    /// assert!(model.geometry_node("ROOT").is_some());
    /// # Ok::<(), nwnrs_types::mdl::ModelError>(())
    /// ```
    pub fn geometry_node(&self, name: &str) -> Option<&AsciiNode> {
        self.geometry
            .iter()
            .filter_map(AsciiBodyItem::as_node)
            .find(|node| node.name.eq_ignore_ascii_case(name))
    }

    /// Iterates geometry nodes in source order.
    ///
    /// # Examples
    ///
    /// ```
    /// let model = nwnrs_types::mdl::parse_ascii_model("beginmodelgeom demo\nnode dummy root\nendnode\nendmodelgeom demo\ndonemodel demo\n")?;
    /// assert_eq!(model.geometry_nodes().count(), 1);
    /// # Ok::<(), nwnrs_types::mdl::ModelError>(())
    /// ```
    pub fn geometry_nodes(&self) -> impl Iterator<Item = &AsciiNode> {
        self.geometry.iter().filter_map(AsciiBodyItem::as_node)
    }

    /// Returns the first animation named `name`, case-insensitively.
    ///
    /// # Examples
    ///
    /// ```
    /// let model = nwnrs_types::mdl::parse_ascii_model("beginmodelgeom demo\nendmodelgeom demo\nnewanim idle demo\ndoneanim idle demo\ndonemodel demo\n")?;
    /// assert!(model.animation("IDLE").is_some());
    /// # Ok::<(), nwnrs_types::mdl::ModelError>(())
    /// ```
    #[must_use]
    pub fn animation(&self, name: &str) -> Option<&AsciiAnimation> {
        self.animations
            .iter()
            .find(|animation| animation.name.eq_ignore_ascii_case(name))
    }

    /// Renders the parsed ASCII model as logical Rust text using canonical
    /// indentation.
    ///
    /// This is not the authoritative byte serialization for byte-transparent
    /// model input: characters representing original bytes from `0x80` through
    /// `0xff` are encoded differently by ordinary UTF-8 conversion. Use
    /// [`write_ascii_model`] when producing an MDL payload.
    ///
    /// # Examples
    ///
    /// ```
    /// let model = nwnrs_types::mdl::parse_ascii_model("beginmodelgeom demo\nendmodelgeom demo\ndonemodel demo\n")?;
    /// assert!(model.to_text().contains("beginmodelgeom demo"));
    /// # Ok::<(), nwnrs_types::mdl::ModelError>(())
    /// ```
    #[must_use]
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

    /// Reads an ASCII MDL model from disk.
    ///
    /// # Errors
    ///
    /// Returns [`ModelError`] if the file cannot be opened or parsed.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// let model = nwnrs_types::mdl::AsciiModel::from_file("model.mdl")?;
    /// assert!(!model.geometry_name.is_empty());
    /// # Ok::<(), nwnrs_types::mdl::ModelError>(())
    /// ```
    pub fn from_file(path: impl AsRef<Path>) -> ModelResult<Self> {
        let mut file = File::open(path.as_ref())?;
        read_ascii_model(&mut file)
    }

    /// Reads an ASCII MDL model from a [`Res`].
    ///
    /// # Errors
    ///
    /// Returns [`ModelError`] if the resource is not an MDL type or parsing
    /// fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use nwnrs_types::resman::{CachePolicy, Res};
    /// fn parse(res: &Res) -> nwnrs_types::mdl::ModelResult<nwnrs_types::mdl::AsciiModel> {
    ///     nwnrs_types::mdl::AsciiModel::from_res(res, CachePolicy::Use)
    /// }
    /// ```
    pub fn from_res(res: &Res, cache_policy: CachePolicy) -> ModelResult<Self> {
        if res.resref().res_type() != MODEL_RES_TYPE {
            return Err(ModelError::msg(format!(
                "expected mdl resource, got {}",
                res.resref()
            )));
        }

        let bytes = res.read_all(cache_policy)?;
        parse_ascii_model_bytes(&bytes)
    }
}

impl Model {
    /// Parses the raw payload as an ASCII MDL model using Latin-1 byte mapping.
    ///
    /// # Errors
    ///
    /// Returns [`ModelError`] if the bytes cannot be parsed as ASCII MDL.
    pub fn parse_ascii(&self) -> ModelResult<AsciiModel> {
        parse_ascii_model_bytes(self.bytes())
    }
}

/// Parses an ASCII MDL model from raw text.
///
/// # Errors
///
/// Returns [`ModelError`] if the text cannot be parsed as a valid ASCII MDL
/// model.
///
/// # Examples
///
/// ```
/// let model = nwnrs_types::mdl::parse_ascii_model(
///     "\
/// newmodel demo
/// setsupermodel demo null
/// classification character
/// setanimationscale 1
/// beginmodelgeom demo
/// node dummy demo
///   parent null
/// endnode
/// endmodelgeom demo
/// newanim idle demo
///   length 1
///   node dummy rootdummy
///     parent demo
///   endnode
/// doneanim idle demo
/// donemodel demo
/// ",
/// )?;
/// assert_eq!(model.geometry_name, "demo");
/// assert!(model.animation("idle").is_some());
/// # Ok::<(), nwnrs_types::mdl::ModelError>(())
/// ```
pub fn parse_ascii_model(text: &str) -> ModelResult<AsciiModel> {
    Parser::new(text).parse_model()
}

/// Controls compiled-to-ASCII lowering.
///
/// # Examples
///
/// ```
/// let options = nwnrs_types::mdl::BinaryToAsciiOptions { embed_original_binary: true };
/// assert!(options.embed_original_binary);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BinaryToAsciiOptions {
    /// Embed the complete original binary payload in ASCII comments so an
    /// unchanged model can later be restored byte-for-byte.
    ///
    /// This is disabled by default because it substantially increases output
    /// size and the metadata has no meaning to other MDL tools.
    pub embed_original_binary: bool,
}

/// Lowers a compiled binary model into canonical ASCII MDL without embedding
/// the original binary payload.
///
/// # Errors
///
/// Returns [`ModelError`] if the binary model cannot be lowered.
///
/// # Examples
///
/// ```no_run
/// # use nwnrs_types::mdl::{AsciiModel, BinaryModel, ModelResult};
/// fn lower(model: &BinaryModel) -> ModelResult<AsciiModel> {
///     nwnrs_types::mdl::lower_binary_model_to_ascii(model)
/// }
/// ```
pub fn lower_binary_model_to_ascii(model: &crate::mdl::BinaryModel) -> ModelResult<AsciiModel> {
    lower_binary_model_to_ascii_with_options(model, BinaryToAsciiOptions::default())
}

/// Lowers a compiled binary model into canonical ASCII MDL with explicit
/// metadata options.
///
/// # Errors
///
/// Returns [`ModelError`] if the binary model cannot be lowered.
///
/// # Examples
///
/// ```no_run
/// # use nwnrs_types::mdl::{AsciiModel, BinaryModel, BinaryToAsciiOptions, ModelResult};
/// fn lower(model: &BinaryModel) -> ModelResult<AsciiModel> {
///     nwnrs_types::mdl::lower_binary_model_to_ascii_with_options(
///         model, BinaryToAsciiOptions { embed_original_binary: true },
///     )
/// }
/// ```
pub fn lower_binary_model_to_ascii_with_options(
    model: &crate::mdl::BinaryModel,
    options: BinaryToAsciiOptions,
) -> ModelResult<AsciiModel> {
    let semantic = crate::mdl::lower_binary_model(model)?;
    lower_semantic_model_to_ascii(
        &semantic,
        options
            .embed_original_binary
            .then_some(model.original_bytes()),
    )
}

/// Restores the original compiled payload embedded in canonical ASCII produced
/// by [`lower_binary_model_to_ascii_with_options`].
///
/// This currently supports canonical ASCII produced by
/// [`lower_binary_model_to_ascii_with_options`] with
/// [`BinaryToAsciiOptions::embed_original_binary`] enabled.
///
/// # Errors
///
/// Returns [`ModelError`] if the model has no embedded compiled source bytes.
///
/// # Examples
///
/// ```no_run
/// # use nwnrs_types::mdl::{AsciiModel, Model, ModelResult};
/// fn restore(model: &AsciiModel) -> ModelResult<Model> {
///     nwnrs_types::mdl::restore_compiled_model(model)
/// }
/// ```
pub fn restore_compiled_model(model: &AsciiModel) -> ModelResult<Model> {
    let bytes = decode_compiled_source_bytes(&model.prefix).ok_or_else(|| {
        ModelError::msg(
            "compiled-payload restoration requires canonical output from \
             lower_binary_model_to_ascii",
        )
    })?;
    let binary = crate::mdl::parse_binary_model_bytes(&bytes)?;
    let canonical = lower_binary_model_to_ascii_with_options(
        &binary,
        BinaryToAsciiOptions {
            embed_original_binary: true,
        },
    )?;
    if canonical != *model {
        return Err(ModelError::msg(
            "cannot restore compiled payload from edited ASCII; use compile_ascii_model to build \
             a new compiled MDL",
        ));
    }
    Ok(Model::new(bytes))
}

pub(crate) fn parse_ascii_model_bytes(bytes: &[u8]) -> ModelResult<AsciiModel> {
    let text = text::decode_model_text(bytes);
    parse_ascii_model(&text)
}

/// Reads an ASCII MDL model from `reader`.
///
/// # Errors
///
/// Returns [`ModelError`] if the data cannot be read or parsed as ASCII MDL.
///
/// # Examples
///
/// ```
/// let mut source = "beginmodelgeom demo\nendmodelgeom demo\ndonemodel demo\n".as_bytes();
/// let model = nwnrs_types::mdl::read_ascii_model(&mut source)?;
/// assert_eq!(model.geometry_name, "demo");
/// # Ok::<(), nwnrs_types::mdl::ModelError>(())
/// ```
#[instrument(level = "debug", skip_all, err)]
pub fn read_ascii_model<R: Read>(reader: &mut R) -> ModelResult<AsciiModel> {
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes)?;
    parse_ascii_model_bytes(&bytes)
}

/// Writes a parsed ASCII MDL model using canonical indentation.
///
/// # Errors
///
/// Returns [`ModelError`] if writing to the output stream fails.
///
/// # Examples
///
/// ```
/// let model = nwnrs_types::mdl::parse_ascii_model(
///     "\
/// newmodel demo
/// setsupermodel demo null
/// classification character
/// setanimationscale 1
/// beginmodelgeom demo
/// node dummy demo
///   parent null
/// endnode
/// endmodelgeom demo
/// donemodel demo
/// ",
/// )?;
/// let mut bytes = Vec::new();
/// nwnrs_types::mdl::write_ascii_model(&mut bytes, &model)?;
/// let text = String::from_utf8(bytes).unwrap();
/// assert!(text.contains("beginmodelgeom demo"));
/// # Ok::<(), nwnrs_types::mdl::ModelError>(())
/// ```
#[instrument(level = "debug", skip_all, err, fields(geometry_name = %model.geometry_name))]
pub fn write_ascii_model<W: Write>(writer: &mut W, model: &AsciiModel) -> ModelResult<()> {
    writer.write_all(&text::encode_model_text(&model.to_text()))?;
    Ok(())
}

pub(crate) fn lower_semantic_model_to_ascii(
    model: &SemanticModel,
    compiled_source_bytes: Option<&[u8]>,
) -> ModelResult<AsciiModel> {
    crate::mdl::semantic::ensure_ascii_representable(model)?;
    let mut prefix = Vec::new();
    if let Some(bytes) = compiled_source_bytes {
        prefix.extend(compiled_source_comments(bytes));
    }
    prefix.push(AsciiElement::Comment("#MAXMODEL ASCII".to_string()));
    prefix.extend(header_elements(&model.header));

    let mut geometry = model
        .nodes
        .iter()
        .map(|node| AsciiBodyItem::Node(semantic_node_to_ascii(node)))
        .collect::<Vec<_>>();
    geometry.extend(
        model
            .geometry_extras
            .iter()
            .cloned()
            .map(AsciiBodyItem::Element),
    );

    let animations = model
        .animations
        .iter()
        .map(semantic_animation_to_ascii)
        .collect::<Vec<_>>();
    let between_animations = if animations.len() > 1 {
        vec![Vec::new(); animations.len() - 1]
    } else {
        Vec::new()
    };

    Ok(AsciiModel {
        prefix,
        geometry_name: model.geometry_name.clone(),
        geometry,
        between_geometry_and_animations: model.between_geometry_and_animations.clone(),
        animations,
        between_animations,
        suffix: model.suffix.clone(),
        done_model_name: model.geometry_name.clone(),
    })
}

fn compiled_source_comments(bytes: &[u8]) -> Vec<AsciiElement> {
    let mut comments = vec![AsciiElement::Comment(COMPILED_SOURCE_BEGIN.to_string())];
    let hex = encode_hex(bytes);
    for chunk in hex.as_bytes().chunks(HEX_CHUNK_LEN) {
        comments.push(AsciiElement::Comment(format!(
            "{COMPILED_SOURCE_HEX_PREFIX}{}",
            String::from_utf8_lossy(chunk)
        )));
    }
    comments.push(AsciiElement::Comment(COMPILED_SOURCE_END.to_string()));
    comments
}

fn decode_compiled_source_bytes(prefix: &[AsciiElement]) -> Option<Vec<u8>> {
    let mut in_block = false;
    let mut hex = String::new();

    for element in prefix {
        let AsciiElement::Comment(comment) = element else {
            continue;
        };
        if comment == COMPILED_SOURCE_BEGIN {
            in_block = true;
            continue;
        }
        if comment == COMPILED_SOURCE_END {
            return decode_hex(&hex).ok();
        }
        if in_block && let Some(chunk) = comment.strip_prefix(COMPILED_SOURCE_HEX_PREFIX) {
            hex.push_str(chunk.trim());
        }
    }

    None
}

fn header_elements(header: &SemanticHeader) -> Vec<AsciiElement> {
    let mut elements = Vec::new();
    elements.push(statement("newmodel", vec![header.model_name.clone()]));
    elements.push(statement(
        "setsupermodel",
        vec![
            header.model_name.clone(),
            header
                .supermodel
                .clone()
                .unwrap_or_else(|| "NULL".to_string()),
        ],
    ));
    if let Some(classification) = &header.classification {
        elements.push(statement(
            "classification",
            vec![classification_token(classification)],
        ));
    }
    if let Some(scale) = header.animation_scale {
        elements.push(statement("setanimationscale", vec![format_scalar(scale)]));
    }
    if let Some(ignore_fog) = header.ignore_fog {
        elements.push(statement("ignorefog", vec![ignore_fog.to_string()]));
    }
    elements.push(AsciiElement::Comment("#MAXGEOM  ASCII".to_string()));
    elements
}

fn semantic_animation_to_ascii(animation: &SemanticAnimation) -> AsciiAnimation {
    let mut body = Vec::new();
    for comment in &animation.comments {
        body.push(AsciiBodyItem::Element(AsciiElement::Comment(
            comment.clone(),
        )));
    }
    if let Some(length) = animation.length {
        body.push(element_statement("length", vec![format_scalar(length)]));
    }
    if let Some(transtime) = animation.transtime {
        body.push(element_statement(
            "transtime",
            vec![format_scalar(transtime)],
        ));
    }
    if let Some(animroot) = &animation.animroot {
        body.push(element_statement("animroot", vec![animroot.clone()]));
    }
    for event in &animation.events {
        body.push(element_statement(
            "event",
            vec![format_scalar(event.time), event.name.clone()],
        ));
    }
    body.extend(animation.extras.iter().cloned().map(AsciiBodyItem::Element));
    body.extend(
        animation
            .nodes
            .iter()
            .map(|node| AsciiBodyItem::Node(animation_node_to_ascii(node))),
    );

    AsciiAnimation {
        name: animation.name.clone(),
        model_name: animation.model_name.clone(),
        body,
    }
}

fn semantic_node_to_ascii(node: &SemanticNode) -> AsciiNode {
    let mut entries = semantic_common_entries(
        &node.comments,
        node.part_number,
        node.parent.as_deref(),
        node.position,
        node.orientation,
        node.scale,
        node.color,
        node.radius,
        node.center,
        node.wirecolor,
    );
    entries.extend(material_entries(&node.material));
    if let Some(mesh) = &node.mesh {
        entries.extend(mesh_entries(mesh));
    }
    if let Some(light) = &node.light {
        entries.extend(light_entries(light));
    }
    if let Some(emitter) = &node.emitter {
        entries.extend(emitter_entries(emitter));
    }
    if let Some(dangly) = &node.dangly {
        entries.extend(dangly_entries(dangly));
    }
    if let Some(reference) = &node.reference {
        entries.extend(reference_entries(reference));
    }
    if let Some(sample_period) = node.sample_period {
        entries.push(statement(
            "sampleperiod",
            vec![format_scalar(sample_period)],
        ));
    }
    entries.extend(node.extras.iter().cloned());

    AsciiNode {
        node_type: node.node_type.clone(),
        name: node.name.clone(),
        entries,
    }
}

fn animation_node_to_ascii(node: &SemanticAnimationNode) -> AsciiNode {
    let mut entries = semantic_common_entries(
        &node.comments,
        node.part_number,
        node.parent.as_deref(),
        node.position,
        node.orientation,
        node.scale,
        node.color,
        node.radius,
        None,
        None,
    );
    if let Some(alpha) = node.alpha {
        entries.push(statement("alpha", vec![format_scalar(alpha)]));
    }
    if let Some(color) = node.self_illum_color {
        entries.push(statement("selfillumcolor", format_vec3(color)));
    }
    if let Some(multiplier) = node.multiplier {
        entries.push(statement("multiplier", vec![format_scalar(multiplier)]));
    }
    if let Some(value) = node.shadow_radius {
        entries.push(statement("shadowradius", vec![format_scalar(value)]));
    }
    if let Some(value) = node.vertical_displacement {
        entries.push(statement(
            "verticaldisplacement",
            vec![format_scalar(value)],
        ));
    }
    key_entries(
        &mut entries,
        &controller_key_name(node, "position"),
        &node.position_keys,
        vec3_key_row,
    );
    key_entries(
        &mut entries,
        &controller_key_name(node, "orientation"),
        &node.orientation_keys,
        vec4_key_row,
    );
    key_entries(
        &mut entries,
        &controller_key_name(node, "scale"),
        &node.scale_keys,
        scalar_key_row,
    );
    key_entries(
        &mut entries,
        &controller_key_name(node, "color"),
        &node.color_keys,
        vec3_key_row,
    );
    key_entries(
        &mut entries,
        &controller_key_name(node, "radius"),
        &node.radius_keys,
        scalar_key_row,
    );
    key_entries(
        &mut entries,
        &controller_key_name(node, "alpha"),
        &node.alpha_keys,
        scalar_key_row,
    );
    key_entries(
        &mut entries,
        &controller_key_name(node, "selfillumcolor"),
        &node.self_illum_color_keys,
        vec3_key_row,
    );
    key_entries(
        &mut entries,
        &controller_key_name(node, "multiplier"),
        &node.multiplier_keys,
        scalar_key_row,
    );
    key_entries(
        &mut entries,
        &controller_key_name(node, "shadowradius"),
        &node.shadow_radius_keys,
        scalar_key_row,
    );
    key_entries(
        &mut entries,
        &controller_key_name(node, "verticaldisplacement"),
        &node.vertical_displacement_keys,
        scalar_key_row,
    );
    for controller in &node.emitter_controllers {
        entries.push(emitter_controller_entry(controller));
    }
    if let Some(dangly) = &node.dangly {
        entries.extend(dangly_entries(dangly));
    }
    if let Some(sample_period) = node.sample_period {
        entries.push(statement(
            "sampleperiod",
            vec![format_scalar(sample_period)],
        ));
    }
    if !node.faces.is_empty() {
        entries.push(payload_statement(
            "faces",
            node.faces.iter().map(face_row).collect(),
        ));
    }
    if !node.animverts.is_empty() {
        entries.push(payload_statement(
            "animverts",
            node.animverts
                .iter()
                .map(|value| format_vec3(*value))
                .collect(),
        ));
    }
    if !node.animtverts.is_empty() {
        entries.push(payload_statement(
            "animtverts",
            node.animtverts
                .iter()
                .map(|value| format_vec2(*value))
                .collect(),
        ));
    }
    entries.extend(node.extras.iter().cloned());

    AsciiNode {
        node_type: node.node_type.clone(),
        name: node.name.clone(),
        entries,
    }
}

fn semantic_common_entries(
    comments: &[String],
    part_number: Option<i32>,
    parent: Option<&str>,
    position: Option<[f32; 3]>,
    orientation: Option<[f32; 4]>,
    scale: Option<f32>,
    color: Option<[f32; 3]>,
    radius: Option<f32>,
    center: Option<[f32; 3]>,
    wirecolor: Option<[f32; 3]>,
) -> Vec<AsciiElement> {
    let mut entries = Vec::new();
    for comment in comments {
        entries.push(AsciiElement::Comment(comment.clone()));
    }
    if let Some(part_number) = part_number {
        entries.push(AsciiElement::Comment(format!("#part-number {part_number}")));
    }
    entries.push(statement(
        "parent",
        vec![parent.unwrap_or("NULL").to_string()],
    ));
    if let Some(position) = position {
        entries.push(statement("position", format_vec3(position)));
    }
    if let Some(orientation) = orientation {
        entries.push(statement("orientation", format_vec4(orientation)));
    }
    if let Some(scale) = scale {
        entries.push(statement("scale", vec![format_scalar(scale)]));
    }
    if let Some(color) = color {
        entries.push(statement("color", format_vec3(color)));
    }
    if let Some(radius) = radius {
        entries.push(statement("radius", vec![format_scalar(radius)]));
    }
    if let Some(center) = center {
        entries.push(statement("center", format_vec3(center)));
    }
    if let Some(wirecolor) = wirecolor {
        entries.push(statement("wirecolor", format_vec3(wirecolor)));
    }
    entries
}

fn material_entries(material: &SemanticMaterial) -> Vec<AsciiElement> {
    let mut entries = Vec::new();
    push_bool_entry(&mut entries, "render", material.render);
    push_bool_entry(&mut entries, "shadow", material.shadow);
    push_i32_entry(&mut entries, "beaming", material.beaming);
    push_i32_entry(&mut entries, "inheritcolor", material.inherit_color);
    push_i32_entry(&mut entries, "tilefade", material.tilefade);
    push_i32_entry(&mut entries, "rotatetexture", material.rotate_texture);
    push_i32_entry(&mut entries, "lightmapped", material.light_mapped);
    push_i32_entry(&mut entries, "transparencyhint", material.transparency_hint);
    push_f32_entry(&mut entries, "shininess", material.shininess);
    push_f32_entry(&mut entries, "alpha", material.alpha);
    push_vec3_entry(&mut entries, "ambient", material.ambient);
    push_vec3_entry(&mut entries, "diffuse", material.diffuse);
    push_vec3_entry(&mut entries, "specular", material.specular);
    push_vec3_entry(&mut entries, "selfillumcolor", material.self_illum_color);
    push_string_entry(
        &mut entries,
        "materialname",
        material.material_name.as_deref(),
    );
    push_string_entry(&mut entries, "renderhint", material.render_hint.as_deref());
    push_string_entry(&mut entries, "bitmap", material.bitmap.as_deref());
    let mut textures = material.textures.clone();
    textures.sort_by_key(|binding| binding.index);
    for SemanticTextureBinding {
        index,
        name,
    } in textures
    {
        entries.push(statement(&format!("texture{index}"), vec![name]));
    }
    entries
}

fn mesh_entries(mesh: &SemanticMesh) -> Vec<AsciiElement> {
    let mut entries = Vec::new();
    if !mesh.vertices.is_empty() {
        entries.push(payload_statement(
            "verts",
            mesh.vertices
                .iter()
                .map(|value| format_vec3(*value))
                .collect(),
        ));
    }
    if !mesh.faces.is_empty() {
        entries.push(payload_statement(
            "faces",
            mesh.faces.iter().map(face_row).collect(),
        ));
    }
    let mut uv_layers = mesh.uv_layers.clone();
    uv_layers.sort_by_key(|layer| layer.index);
    for layer in uv_layers {
        entries.push(payload_statement(
            &uv_keyword(layer.index),
            layer
                .coordinates
                .iter()
                .map(|value| format_vec3([value[0], value[1], 0.0]))
                .collect(),
        ));
    }
    if !mesh.normals.is_empty() {
        entries.push(payload_statement(
            "normals",
            mesh.normals
                .iter()
                .map(|value| format_vec3(*value))
                .collect(),
        ));
    }
    if !mesh.tangents.is_empty() {
        entries.push(payload_statement(
            "tangents",
            mesh.tangents
                .iter()
                .map(|row| format_f32_row(row))
                .collect(),
        ));
    }
    if !mesh.colors.is_empty() {
        entries.push(payload_statement(
            "colors",
            mesh.colors.iter().map(|row| format_f32_row(row)).collect(),
        ));
    }
    if !mesh.weights.is_empty() {
        entries.push(payload_statement(
            "weights",
            mesh.weights.iter().map(|row| weight_row(row)).collect(),
        ));
    }
    if !mesh.constraints.is_empty() {
        entries.push(payload_statement(
            "constraints",
            mesh.constraints
                .iter()
                .map(|row| format_f32_row(row))
                .collect(),
        ));
    }
    if !mesh.multimaterial.is_empty() {
        entries.push(payload_statement(
            "multimaterial",
            mesh.multimaterial
                .iter()
                .map(|value| vec![value.clone()])
                .collect(),
        ));
    }
    if !mesh.texture_names.is_empty() {
        entries.push(payload_statement(
            "texturenames",
            mesh.texture_names
                .iter()
                .map(|value| vec![value.clone()])
                .collect(),
        ));
    }
    entries
}

fn light_entries(light: &SemanticLight) -> Vec<AsciiElement> {
    let mut entries = Vec::new();
    push_f32_entry(&mut entries, "multiplier", light.multiplier);
    push_i32_entry(&mut entries, "ambientonly", light.ambient_only);
    push_i32_entry(&mut entries, "ndynamictype", light.n_dynamic_type);
    push_i32_entry(&mut entries, "isdynamic", light.is_dynamic);
    push_i32_entry(&mut entries, "affectdynamic", light.affect_dynamic);
    push_i32_entry(&mut entries, "negativelight", light.negative_light);
    push_i32_entry(&mut entries, "lightpriority", light.light_priority);
    push_i32_entry(&mut entries, "fadinglight", light.fading_light);
    push_i32_entry(&mut entries, "lensflares", light.lens_flares);
    push_f32_entry(&mut entries, "flareradius", light.flare_radius);
    push_f32_entry(&mut entries, "shadowradius", light.shadow_radius);
    push_f32_entry(
        &mut entries,
        "verticaldisplacement",
        light.vertical_displacement,
    );
    if !light.flare_textures.is_empty() {
        entries.push(payload_statement(
            "texturenames",
            light
                .flare_textures
                .iter()
                .map(|value| vec![value.clone()])
                .collect(),
        ));
    }
    if !light.flare_sizes.is_empty() {
        entries.push(payload_statement(
            "flaresizes",
            light
                .flare_sizes
                .iter()
                .map(|value| vec![format_scalar(*value)])
                .collect(),
        ));
    }
    if !light.flare_positions.is_empty() {
        entries.push(payload_statement(
            "flarepositions",
            light
                .flare_positions
                .iter()
                .map(|value| vec![format_scalar(*value)])
                .collect(),
        ));
    }
    if !light.flare_color_shifts.is_empty() {
        entries.push(payload_statement(
            "flarecolorshifts",
            light
                .flare_color_shifts
                .iter()
                .map(|value| format_vec3(*value))
                .collect(),
        ));
    }
    entries
}

fn emitter_entries(emitter: &SemanticEmitter) -> Vec<AsciiElement> {
    let mut entries = Vec::new();
    push_f32_entry(&mut entries, "xsize", emitter.x_size);
    push_f32_entry(&mut entries, "ysize", emitter.y_size);
    for SemanticEmitterProperty {
        name,
        values,
    } in &emitter.properties
    {
        entries.push(statement(
            name,
            values.iter().map(format_property_value).collect(),
        ));
    }
    entries
}

fn dangly_entries(dangly: &SemanticDangly) -> Vec<AsciiElement> {
    let mut entries = Vec::new();
    push_f32_entry(&mut entries, "displacement", dangly.displacement);
    push_f32_entry(&mut entries, "tightness", dangly.tightness);
    push_f32_entry(&mut entries, "period", dangly.period);
    entries
}

fn emitter_controller_entry(controller: &SemanticEmitterController) -> AsciiElement {
    let suffix = if controller.bezier_keyed {
        "bezierkey"
    } else {
        "key"
    };
    payload_statement(
        &format!("{}{suffix}", controller.name),
        controller
            .keys
            .iter()
            .map(|key| {
                let mut row = Vec::with_capacity(key.values.len() + 1);
                row.push(format_scalar(key.time));
                row.extend(key.values.iter().map(|value| format_scalar(*value)));
                row
            })
            .collect(),
    )
}

fn controller_key_name(node: &SemanticAnimationNode, name: &str) -> String {
    if node
        .bezier_controllers
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(name))
    {
        format!("{name}bezierkey")
    } else {
        format!("{name}key")
    }
}

fn reference_entries(reference: &SemanticReference) -> Vec<AsciiElement> {
    let mut entries = Vec::new();
    push_string_entry(&mut entries, "refmodel", reference.model.as_deref());
    push_i32_entry(&mut entries, "reattachable", reference.reattachable);
    entries
}

fn key_entries<T, F>(entries: &mut Vec<AsciiElement>, keyword: &str, keys: &[T], formatter: F)
where
    F: Fn(&T) -> Vec<String>,
{
    if !keys.is_empty() {
        entries.push(payload_statement(
            keyword,
            keys.iter().map(formatter).collect(),
        ));
    }
}

fn vec3_key_row(key: &Vec3Key) -> Vec<String> {
    let mut row = vec![format_scalar(key.time)];
    row.extend(format_vec3(key.value));
    row
}

fn vec4_key_row(key: &Vec4Key) -> Vec<String> {
    let mut row = vec![format_scalar(key.time)];
    row.extend(format_vec4(key.value));
    row
}

fn scalar_key_row(key: &ScalarKey) -> Vec<String> {
    vec![format_scalar(key.time), format_scalar(key.value)]
}

fn face_row(face: &SemanticFace) -> Vec<String> {
    vec![
        face.vertex_indices[0].to_string(),
        face.vertex_indices[1].to_string(),
        face.vertex_indices[2].to_string(),
        face.group.to_string(),
        face.uv_indices[0].to_string(),
        face.uv_indices[1].to_string(),
        face.uv_indices[2].to_string(),
        face.material_index.to_string(),
    ]
}

fn weight_row(row: &[SemanticSkinWeight]) -> Vec<String> {
    let mut values = Vec::new();
    for weight in row {
        values.push(weight.bone.clone());
        values.push(format_scalar(weight.weight));
    }
    values
}

fn format_property_value(value: &SemanticPropertyValue) -> String {
    match value {
        SemanticPropertyValue::Bool(value) => {
            if *value {
                "1".to_string()
            } else {
                "0".to_string()
            }
        }
        SemanticPropertyValue::Int(value) => value.to_string(),
        SemanticPropertyValue::Float(value) => format_scalar(*value),
        SemanticPropertyValue::Text(value) => value.clone(),
    }
}

fn push_bool_entry(entries: &mut Vec<AsciiElement>, keyword: &str, value: Option<bool>) {
    if let Some(value) = value {
        entries.push(statement(
            keyword,
            vec![if value { "1" } else { "0" }.to_string()],
        ));
    }
}

fn push_i32_entry(entries: &mut Vec<AsciiElement>, keyword: &str, value: Option<i32>) {
    if let Some(value) = value {
        entries.push(statement(keyword, vec![value.to_string()]));
    }
}

fn push_f32_entry(entries: &mut Vec<AsciiElement>, keyword: &str, value: Option<f32>) {
    if let Some(value) = value {
        entries.push(statement(keyword, vec![format_scalar(value)]));
    }
}

fn push_vec3_entry(entries: &mut Vec<AsciiElement>, keyword: &str, value: Option<[f32; 3]>) {
    if let Some(value) = value {
        entries.push(statement(keyword, format_vec3(value)));
    }
}

fn push_string_entry(entries: &mut Vec<AsciiElement>, keyword: &str, value: Option<&str>) {
    if let Some(value) = value {
        entries.push(statement(keyword, vec![value.to_string()]));
    }
}

fn element_statement(keyword: &str, arguments: Vec<String>) -> AsciiBodyItem {
    AsciiBodyItem::Element(statement(keyword, arguments))
}

fn statement(keyword: &str, arguments: Vec<String>) -> AsciiElement {
    AsciiElement::Statement(AsciiStatement::new(keyword, arguments))
}

fn payload_statement(keyword: &str, rows: Vec<Vec<String>>) -> AsciiElement {
    AsciiElement::Statement(AsciiStatement::with_payload(
        keyword,
        Vec::new(),
        AsciiPayloadKind::Counted,
        rows,
    ))
}

fn uv_keyword(index: usize) -> String {
    if index == 0 {
        "tverts".to_string()
    } else {
        format!("tverts{index}")
    }
}

fn classification_token(value: &ModelClassification) -> String {
    match value {
        ModelClassification::Character => "character".to_string(),
        ModelClassification::Tile => "tile".to_string(),
        ModelClassification::Door => "door".to_string(),
        ModelClassification::Effect => "effect".to_string(),
        ModelClassification::Gui => "gui".to_string(),
        ModelClassification::Item => "item".to_string(),
        ModelClassification::Other(value) => value.clone(),
    }
}

fn format_vec2(value: [f32; 2]) -> Vec<String> {
    vec![format_scalar(value[0]), format_scalar(value[1])]
}

fn format_vec3(value: [f32; 3]) -> Vec<String> {
    vec![
        format_scalar(value[0]),
        format_scalar(value[1]),
        format_scalar(value[2]),
    ]
}

fn format_vec4(value: [f32; 4]) -> Vec<String> {
    vec![
        format_scalar(value[0]),
        format_scalar(value[1]),
        format_scalar(value[2]),
        format_scalar(value[3]),
    ]
}

fn format_f32_row(row: &[f32]) -> Vec<String> {
    row.iter().map(|value| format_scalar(*value)).collect()
}

fn format_scalar(value: f32) -> String {
    value.to_string()
}

fn encode_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let high = byte >> 4;
        let low = byte & 0x0f;
        out.push(char::from_digit(u32::from(high), 16).unwrap_or('0'));
        out.push(char::from_digit(u32::from(low), 16).unwrap_or('0'));
    }
    out
}

fn decode_hex(input: &str) -> Result<Vec<u8>, ()> {
    if !input.len().is_multiple_of(2) {
        return Err(());
    }

    let mut bytes = Vec::with_capacity(input.len() / 2);
    for pair in input.as_bytes().chunks_exact(2) {
        let [high, low] = *pair else {
            return Err(());
        };
        let high = decode_hex_nibble(high)?;
        let low = decode_hex_nibble(low)?;
        bytes.push((high << 4) | low);
    }
    Ok(bytes)
}

fn decode_hex_nibble(value: u8) -> Result<u8, ()> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        b'A'..=b'F' => Ok(value - b'A' + 10),
        _ => Err(()),
    }
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
                self.parse_statement()?;
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
            animations.push(self.parse_animation(&geometry_name)?);
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
                    animations.push(self.parse_animation(&geometry_name)?);
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

    fn parse_animation(&mut self, geometry_name: &str) -> ModelResult<AsciiAnimation> {
        let new_anim = self.parse_statement()?;
        if !new_anim.keyword_is("newanim") {
            return Err(ModelError::msg("animation must start with newanim"));
        }
        let name = new_anim
            .argument(0)
            .ok_or_else(|| ModelError::msg("newanim requires an animation name"))?
            .to_string();
        let model_name = if let Some(model_name) = new_anim.argument(1) {
            model_name.to_string()
        } else if self.peek_meaningful().is_some_and(|line| {
            let tokens = split_tokens(line.trim());
            tokens.len() == 1
                && tokens
                    .first()
                    .is_some_and(|token| token.eq_ignore_ascii_case(geometry_name))
        }) {
            self.parse_statement()?.keyword
        } else {
            geometry_name.to_string()
        };

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
                if done_anim.argument(1).is_none()
                    && self.peek_meaningful().is_some_and(|line| {
                        let tokens = split_tokens(line.trim());
                        tokens.len() == 1
                            && tokens
                                .first()
                                .is_some_and(|token| token.eq_ignore_ascii_case(geometry_name))
                    })
                {
                    self.parse_statement()?;
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
            if keyword_of(line).is_some_and(is_node_terminator) {
                let endnode = self.parse_statement()?;
                if !is_node_terminator(&endnode.keyword) {
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
            return Ok(AsciiElement::Comment(comment.trim().to_string()));
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
        if keyword_lower == "aabb" && !raw_arguments.is_empty() {
            let mut payload_rows = vec![raw_arguments.to_vec()];
            while self
                .peek_meaningful()
                .is_some_and(|next| indentation_of(next) > indent)
            {
                self.skip_blank_lines();
                let row = self
                    .next()
                    .ok_or_else(|| ModelError::msg("AABB payload ended unexpectedly"))?;
                if row.trim_start().starts_with('#') {
                    return Err(ModelError::msg(
                        "comments inside indented AABB payloads are not supported",
                    ));
                }
                payload_rows.push(split_tokens(row.trim()));
            }
            return Ok(AsciiStatement::with_payload(
                keyword.clone(),
                Vec::new(),
                AsciiPayloadKind::Indented,
                payload_rows,
            ));
        }
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

            if self.peek_meaningful().is_some_and(|next| {
                indentation_of(next) > indent
                    || (keyword_lower.ends_with("key") && looks_like_endlist_payload_line(next))
            }) {
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
        let remaining_lines = self.lines.len().saturating_sub(self.index);
        if count > remaining_lines {
            return Err(ModelError::msg(format!(
                "payload declares {count} rows but only {remaining_lines} source lines remain"
            )));
        }
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

fn is_node_terminator(keyword: &str) -> bool {
    keyword.eq_ignore_ascii_case("endnode") || keyword.eq_ignore_ascii_case("endnodeendnode")
}

fn looks_like_endlist_payload_line(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.eq_ignore_ascii_case("endlist")
        || trimmed
            .split_whitespace()
            .next()
            .is_some_and(|token| parse_legacy_f32(token).is_some())
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
        Some(AsciiPayloadKind::Indented) => {
            let mut rows = statement.payload_rows.iter();
            let first = rows.next().map(Vec::as_slice).unwrap_or(&[]);
            let arguments = first.iter().map(String::as_str).collect::<Vec<_>>();
            write_statement_line(out, indent, &statement.keyword, &arguments);
            for row in rows {
                write_row_line(out, indent + 2, row);
            }
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

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use crate::mdl::{AsciiElement, AsciiPayloadKind, parse_ascii_model, write_ascii_model};

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

    #[test]
    fn counted_payload_rejects_impossible_allocation_before_reserving() {
        let source =
            "newmodel demo\nbeginmodelgeom demo\nnode trimesh demo\nverts 18446744073709551615\n";
        let error = parse_ascii_model(source).unwrap_err();
        assert!(error.to_string().contains("source lines remain"));
    }

    #[test]
    fn indented_aabb_payload_roundtrips_without_an_endlist() {
        let source = "newmodel demo\nbeginmodelgeom demo\nnode aabb walk\n  parent demo\n  aabb 0 \
                      0 0 1 1 1 -1\n    0 0 0 0.5 1 1 0\nendnode\nendmodelgeom demo\ndonemodel \
                      demo\n";
        let model = parse_ascii_model(source).unwrap_or_else(|error| {
            panic!("parse indented AABB sample: {error}");
        });
        let statement = model
            .geometry_node("walk")
            .and_then(|node| node.statement("aabb"))
            .unwrap_or_else(|| panic!("missing indented AABB statement"));
        assert_eq!(statement.payload_kind, Some(AsciiPayloadKind::Indented));
        assert_eq!(statement.payload_rows.len(), 2);
    }
}
