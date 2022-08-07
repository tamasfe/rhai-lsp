#![allow(dead_code)]
use crate::{source::SourceInfo, Hir, IndexMap, IndexSet};
use core::fmt;

slotmap::new_key_type! { pub struct Type; }

impl Type {
    #[must_use]
    pub fn fmt(self, hir: &Hir) -> TypeFormatter {
        TypeFormatter { hir, ty: self }
    }
}

#[derive(Debug, Default, Clone)]
#[non_exhaustive]
pub struct TypeData {
    pub source: SourceInfo,
    pub kind: TypeKind,
}

/// Used to print a type.
pub struct TypeFormatter<'a> {
    hir: &'a Hir,
    ty: Type,
}

impl core::fmt::Display for TypeFormatter<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let data = &self.hir[self.ty];

        match &data.kind {
            TypeKind::Module => f.write_str("module")?,
            TypeKind::Int => f.write_str("int")?,
            TypeKind::Float => f.write_str("float")?,
            TypeKind::Bool => f.write_str("bool")?,
            TypeKind::Char => f.write_str("char")?,
            TypeKind::String => f.write_str("String")?,
            TypeKind::Timestamp => f.write_str("timestamp")?,
            TypeKind::Array(arr) => {
                f.write_str("[")?;
                write!(f, "{}", arr.items.fmt(self.hir))?;
                f.write_str("]")?;
            }
            TypeKind::Object(obj) => {
                f.write_str("#{")?;

                let mut first = true;
                for (name, ty) in &obj.fields {
                    if !first {
                        f.write_str(", ")?;
                    }
                    first = false;

                    write!(f, "{name}: {}", ty.fmt(self.hir))?;
                }
                f.write_str("}")?;
            }
            TypeKind::Union(tys) => {
                let mut first = true;
                for ty in tys {
                    if !first {
                        f.write_str("| ")?;
                    }
                    first = false;

                    write!(f, "{}", ty.fmt(self.hir))?;
                }
            }
            TypeKind::Void => f.write_str("()")?,
            TypeKind::Fn(func) => {
                if func.is_closure {
                    f.write_str("|")?;
                } else {
                    f.write_str("fn (")?;
                }

                let mut first = true;
                for (name, ty) in &func.params {
                    if !first {
                        f.write_str(", ")?;
                    }
                    first = false;

                    write!(f, "{name}: {}", ty.fmt(self.hir))?;
                }

                if func.is_closure {
                    f.write_str("|")?;
                } else {
                    f.write_str(")")?;
                }

                write!(f, " -> {}", func.ret.fmt(self.hir))?;
            }
            TypeKind::Alias(alias, _) => f.write_str(alias.trim())?,
            TypeKind::Unresolved(ty) => f.write_str(ty.trim())?,
            TypeKind::Never => f.write_str("!")?,
            TypeKind::Unknown => f.write_str("?")?,
        }

        Ok(())
    }
}

// impl TypeData {
//     fn to_writer(&self, hir: &Hir, writer: &mut dyn fmt::Write) -> fmt::Result {
//         match &self.kind {
//             TypeKind::Module => writer.write_str("module")?,
//             TypeKind::Int => writer.write_str("int"),
//             TypeKind::Float => todo!(),
//             TypeKind::Bool => todo!(),
//             TypeKind::Char => todo!(),
//             TypeKind::String => todo!(),
//             TypeKind::Timestamp => todo!(),
//             TypeKind::Array(_) => todo!(),
//             TypeKind::Object(_) => todo!(),
//             TypeKind::Union(_) => todo!(),
//             TypeKind::Void => todo!(),
//             TypeKind::Fn(_) => todo!(),
//             TypeKind::Alias(_, _) => todo!(),
//             TypeKind::Unresolved(_) => todo!(),
//             TypeKind::Never => todo!(),
//             TypeKind::Unknown => todo!(),
//         }

//         Ok(())
//     }
// }

#[derive(Debug, Clone)]
pub enum TypeKind {
    Module,
    Int,
    Float,
    Bool,
    Char,
    String,
    Timestamp,
    Array(Array),
    Object(Object),
    Union(IndexSet<Type>),
    Void,
    Fn(Function),
    Alias(String, Type),
    Unresolved(String),
    Never,
    Unknown,
}

impl TypeKind {
    /// Returns `true` if the type kind is [`Module`].
    ///
    /// [`Module`]: TypeKind::Module
    #[must_use]
    pub fn is_module(&self) -> bool {
        matches!(self, Self::Module)
    }

    /// Returns `true` if the type kind is [`Int`].
    ///
    /// [`Int`]: TypeKind::Int
    #[must_use]
    pub fn is_int(&self) -> bool {
        matches!(self, Self::Int)
    }

    /// Returns `true` if the type kind is [`Float`].
    ///
    /// [`Float`]: TypeKind::Float
    #[must_use]
    pub fn is_float(&self) -> bool {
        matches!(self, Self::Float)
    }

    /// Returns `true` if the type kind is [`Bool`].
    ///
    /// [`Bool`]: TypeKind::Bool
    #[must_use]
    pub fn is_bool(&self) -> bool {
        matches!(self, Self::Bool)
    }

    /// Returns `true` if the type kind is [`Char`].
    ///
    /// [`Char`]: TypeKind::Char
    #[must_use]
    pub fn is_char(&self) -> bool {
        matches!(self, Self::Char)
    }

    /// Returns `true` if the type kind is [`String`].
    ///
    /// [`String`]: TypeKind::String
    #[must_use]
    pub fn is_string(&self) -> bool {
        matches!(self, Self::String)
    }

    /// Returns `true` if the type kind is [`Timestamp`].
    ///
    /// [`Timestamp`]: TypeKind::Timestamp
    #[must_use]
    pub fn is_timestamp(&self) -> bool {
        matches!(self, Self::Timestamp)
    }

    /// Returns `true` if the type kind is [`Array`].
    ///
    /// [`Array`]: TypeKind::Array
    #[must_use]
    pub fn is_array(&self) -> bool {
        matches!(self, Self::Array(..))
    }

    #[must_use]
    pub fn as_array(&self) -> Option<&Array> {
        if let Self::Array(v) = self {
            Some(v)
        } else {
            None
        }
    }

    /// Returns `true` if the type kind is [`Object`].
    ///
    /// [`Object`]: TypeKind::Object
    #[must_use]
    pub fn is_object(&self) -> bool {
        matches!(self, Self::Object(..))
    }

    #[must_use]
    pub fn as_object(&self) -> Option<&Object> {
        if let Self::Object(v) = self {
            Some(v)
        } else {
            None
        }
    }

    /// Returns `true` if the type kind is [`Union`].
    ///
    /// [`Union`]: TypeKind::Union
    #[must_use]
    pub fn is_union(&self) -> bool {
        matches!(self, Self::Union(..))
    }

    #[must_use]
    pub fn as_union(&self) -> Option<&IndexSet<Type>> {
        if let Self::Union(v) = self {
            Some(v)
        } else {
            None
        }
    }

    /// Returns `true` if the type kind is [`Void`].
    ///
    /// [`Void`]: TypeKind::Void
    #[must_use]
    pub fn is_void(&self) -> bool {
        matches!(self, Self::Void)
    }

    /// Returns `true` if the type kind is [`Fn`].
    ///
    /// [`Fn`]: TypeKind::Fn
    #[must_use]
    pub fn is_fn(&self) -> bool {
        matches!(self, Self::Fn(..))
    }

    #[must_use]
    pub fn as_fn(&self) -> Option<&Function> {
        if let Self::Fn(v) = self {
            Some(v)
        } else {
            None
        }
    }

    /// Returns `true` if the type kind is [`Alias`].
    ///
    /// [`Alias`]: TypeKind::Alias
    #[must_use]
    pub fn is_alias(&self) -> bool {
        matches!(self, Self::Alias(..))
    }

    /// Returns `true` if the type kind is [`Unresolved`].
    ///
    /// [`Unresolved`]: TypeKind::Unresolved
    #[must_use]
    pub fn is_unresolved(&self) -> bool {
        matches!(self, Self::Unresolved(..))
    }

    /// Returns `true` if the type kind is [`Never`].
    ///
    /// [`Never`]: TypeKind::Never
    #[must_use]
    pub fn is_never(&self) -> bool {
        matches!(self, Self::Never)
    }

    /// Returns `true` if the type kind is [`Unknown`].
    ///
    /// [`Unknown`]: TypeKind::Unknown
    #[must_use]
    pub fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown)
    }
}

impl Default for TypeKind {
    fn default() -> Self {
        Self::Unknown
    }
}

#[derive(Debug, Clone)]
pub struct Object {
    pub fields: IndexMap<String, Type>,
}

#[derive(Debug, Clone)]
pub struct Array {
    pub items: Type,
}

#[derive(Debug, Clone)]
pub struct Function {
    pub is_closure: bool,
    pub params: Vec<(String, Type)>,
    pub ret: Type,
}