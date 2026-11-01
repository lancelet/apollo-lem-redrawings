//! What messages carry: plain values with public builders. Building a
//! payload is not editing a document, so everything here constructs
//! freely; the checks that make a value valid run in its constructor, and
//! again when a document takes it in.

/// An id type: an interned `Copy` handle by name.
macro_rules! id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        pub struct $name(ustr::Ustr);

        impl $name {
            /// A name.
            pub fn new(name: &str) -> Self {
                Self(ustr::Ustr::from(name))
            }

            /// The name.
            pub fn as_str(&self) -> &str {
                self.0.as_str()
            }
        }

        impl From<&str> for $name {
            fn from(s: &str) -> Self {
                Self::new(s)
            }
        }

        impl From<&$name> for $name {
            fn from(id: &$name) -> Self {
                *id
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

pub mod bounds;
pub mod centre;
pub mod claims;
pub mod dimension;
pub mod drafting;
pub mod form;
pub mod frame;
pub mod gaps;
pub mod geometry;
pub mod hops;
pub mod ink;
pub mod leader;
pub mod lettering;
pub mod marking;
pub mod measure;
pub mod observed;
pub mod params;
pub mod part;
#[cfg(test)]
mod properties;
pub mod provenance;
pub mod relation;
pub mod residuals;
pub mod sketch;
pub mod style;
pub mod text;
pub mod unmodelled;
pub mod view;
pub mod weights;
