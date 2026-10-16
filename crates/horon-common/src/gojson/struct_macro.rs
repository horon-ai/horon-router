/// Declares a Go struct with its wire names. Append `omitempty` after a name for Go's
/// `,omitempty`.
///
/// ```ignore
/// go_struct! {
///     pub struct PayloadResponse("pluginapi.PayloadResponse") {
///         "Body" => body: Bytes,
///         "path" omitempty => path: String,
///     }
/// }
/// ```
#[macro_export]
macro_rules! go_struct {
    (
        $(#[$meta:meta])*
        pub struct $name:ident ($go_type:literal) {
            $( $(#[$fmeta:meta])* $wire:literal $($omit:ident)? => $field:ident : $ty:ty ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Default, PartialEq)]
        pub struct $name {
            $( $(#[$fmeta])* pub $field: $ty, )*
        }

        impl $crate::gojson::GoJson for $name {
            const GO_TYPE: &'static str = $go_type;
            fn encode(&self, out: &mut Vec<u8>) {
                let mut w = $crate::gojson::ObjWriter::begin(out);
                $crate::gojson::GoStruct::encode_fields(self, &mut w);
                w.end();
            }
            fn decode_into(&mut self, v: &$crate::gojson::Node) -> Result<(), $crate::gojson::DecodeError> {
                $crate::gojson::decode_struct_into(self, v)
            }
            fn is_empty(&self) -> bool {
                false
            }
        }

        impl $crate::gojson::GoStruct for $name {
            #[allow(unused_variables)]
            fn encode_fields(&self, w: &mut $crate::gojson::ObjWriter<'_>) {
                $( w.field($wire, &self.$field, $crate::go_struct!(@omit $($omit)?)); )*
            }
            #[allow(unused_variables)]
            fn decode_field(
                &mut self,
                key: &str,
                v: &$crate::gojson::Node,
            ) -> Option<Result<(), $crate::gojson::DecodeError>> {
                $(
                    if key == $wire {
                        return Some($crate::gojson::decode_field_value(&mut self.$field, v, $go_type, $wire));
                    }
                )*
                $(
                    if key.eq_ignore_ascii_case($wire) {
                        return Some($crate::gojson::decode_field_value(&mut self.$field, v, $go_type, $wire));
                    }
                )*
                None
            }
        }
    };
    (@omit omitempty) => { true };
    (@omit) => { false };
}
