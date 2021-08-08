macro_rules! file_kinds {
    (
        $(
            $field_name:tt
        ),+$(,)+
    ) => {
        mod file_kind_module {
            #![allow(non_upper_case_globals, non_camel_case_types, dead_code)]
            use super::*;

            mod consts {
                $(
                    pub const $field_name: u64 = super::murmur_hash64a(stringify!($field_name).as_bytes(), 0);
                )+
                pub const r#mod: u64 = super::murmur_hash64a("mod".as_bytes(), 0);
            }

            mod strings {
                $(
                    pub const $field_name: &str = stringify!($field_name);
                )+
                pub const r#mod: &str = "mod";
            }

            #[cfg_attr(feature = "serde_support", derive(serde::Serialize, serde::Deserialize))]
            #[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
            #[repr(u64)]
            pub enum FileKind {
                $(
                    $field_name = consts::$field_name,
                )+
                r#mod = consts::r#mod,

                /// Unknown file type.
                Unknown,
            }

            impl FileKind {
                pub fn with_hash(hash: u64) -> Self {
                    match hash {
                        $(
                            consts::$field_name => Self::$field_name,
                        )+
                        consts::r#mod => Self::r#mod,
                        _ => Self::Unknown,
                    }
                }

                pub fn with_str(ext: &str) -> Self {
                    match ext {
                        $(
                              strings::$field_name => Self::$field_name,
                        )+
                        strings::r#mod => Self::r#mod,
                        _ => Self::Unknown,
                    }
                }

                pub fn as_str(&self) -> Option<&'static str> {
                    match self {
                        $(
                              Self::$field_name => Some(strings::$field_name),
                        )+
                        Self::r#mod => Some(strings::r#mod),
                        _ => None,
                    }
                }

                /// List of known extensions.
                pub fn extensions() -> &'static [&'static str] {
                    &[
                        $(
                            strings::$field_name,
                        )+
                        "mod",
                    ]
                }
            }
        }
        #[allow(unused_imports)]
        pub use file_kind_module::FileKind as FileKind;
    }
}

