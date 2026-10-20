//! A page of a collection (docs/api.md, 「Wire format」): its items and whether anything stands past the last.

/// A page type of one member type, named for that member in the OpenAPI document.
macro_rules! page {
  ($page:ident, $member:ty, $doc:literal) => {
    #[doc = $doc]
    #[derive(Clone, Debug, PartialEq, ::serde::Serialize, ::serde::Deserialize, ::schemars::JsonSchema)]
    pub struct $page {
      pub items: Vec<$member>,
      /// Whether anything stands past the last item.
      pub more: bool,
    }

    impl $page {
      /// A page holding every member there is.
      pub fn whole(items: Vec<$member>) -> Self {
        Self { items, more: false }
      }
    }
  };
}

pub(crate) use page;
