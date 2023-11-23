/// The base type of all network based strings.
pub type NetString = String;

/// Anything that can be sent over the network. This trait just
/// enforces that is can be converted into a string.
pub trait Networkable {
    fn to_net_str(&self) -> NetString;
}

/// Calls `self.to_string()`
impl Networkable for &str {
    fn to_net_str(&self) -> NetString {
        self.to_string()
    }
}

/// Clones `self` using String::from
impl Networkable for &String {
    fn to_net_str(&self) -> NetString {
        String::from(&**self)
    }
}

/// Any Networkable can be used for `.into()` for NetString's.
impl<'a> Into<NetString> for &'a dyn Networkable {
    fn into(self) -> NetString {
        self.to_net_str()
    }
}

macro_rules! basic_networkable {
    ($t:ident) => {
        /// This implementation just uses format!()
        /// This is because the type is integral.
        impl Networkable for $t {
            fn to_net_str(&self) -> NetString {
                format!("{}", self)
            }
        }
    };
}

basic_networkable!(u8);
basic_networkable!(i8);
basic_networkable!(u16);
basic_networkable!(i16);
basic_networkable!(u32);
basic_networkable!(i32);
basic_networkable!(u64);
basic_networkable!(i64);
basic_networkable!(usize);

// TODO: Add code examples for JsonBuilder.

/// Easy json builder using builder notation. This struct formats data correctly using indentation.
pub struct JsonBuilder {
    data: Vec<String>,
    indent: usize,
    index: usize,
    context: JsonKind,
    tag: JsonKind,
}

/// The kind of json that a `JsonBuilder` is representing.
#[derive(Clone, PartialEq)]
pub enum JsonKind {
    /// A json object.
    Object,
    /// A json array.
    Array,
    /// Not yet known.
    None,
}

impl Default for JsonBuilder {
    fn default() -> Self {
        Self {
            data: Vec::new(),
            indent: 0,
            tag: JsonKind::None,
            index: 0,
            context: JsonKind::None,
        }
    }
}

/// The amount of spaces used while generated json (*using `JsonBuilder`*)
pub const JSON_INDENT: usize = 2;

impl JsonBuilder {
    /// Build a json object. This initializes `JsonBuilder` with the initial braces required for an object.
    ///
    /// **NOTE**: `JsonBuilder::default()` initializes the object without any initial state. If you want
    ///           to create stuff yourself, use that.
    ///
    /// This initializes the object with the tag `JsonKind::Object` and expects further calls to be related to an object.
    pub fn object() -> Self {
        let mut this = Self::default();
        this.tag = JsonKind::Object;
        this.indent = 2;
        // roughly translates to an empty array. Such as:
        // - {
        // -
        // - }
        this.data = vec![String::from("{"), String::new(), String::from("}")];
        // We are within the context of the object from the get-go, so set that here.
        this.context = JsonKind::Object;
        // We are at index 1, due to the object syntax.
        this.index = 1;
        this
    }

    /// Build a json array. This initializes `JsonBuilder` with the initial brackets required for an array.
    ///
    /// **NOTE**: `JsonBuilder::default()` initializes the object without any initial state. If you want
    ///           to create stuff yourself, use that.
    ///
    /// This initializes the object with the tag `JsonKind::Object` and expects further calls to be related to an array.
    pub fn array() -> Self {
        let mut this = Self::default();
        this.tag = JsonKind::Array;
        this.data = vec![String::from("["), String::new(), String::from("]")];
        this.context = JsonKind::Array;
        this.indent = 2;
        this.index = 1;
        this
    }

    pub fn tag(&self) -> JsonKind {
        self.tag.clone()
    }

    /// Set the tag. The tag represents what the base of the object is. For example, if the items reside inside of braces
    /// at the very top-level, the tag would be `JsonKind::Object`.
    /// ## Example
    /// ```
    /// let object = JsonBuilder::object();
    /// assert!(object.tag() == JsonKind::Object);
    ///
    /// let arr = JsonBuilder::array();
    /// assert!(object.tag() == JsonKind::Array);
    /// ```
    ///
    /// This function exists for when using `JsonBuilder::default()`. When using `JsonBuilder::object()` and `JsonBuilder::array()`,
    /// this is automatically initialized. However, `JsonBuilder::default()` initializes it to `JsonKind::None`.
    pub fn with_tag(mut self, tag: JsonKind) -> Self {
        self.tag = tag;
        self
    }

    pub fn with_entry(mut self, key: String, value: impl Networkable) -> Self {
        assert!(self.context == JsonKind::Object);

        self
    }
}
