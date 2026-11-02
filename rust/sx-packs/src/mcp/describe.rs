use sx_kernel::json::Json;
use sx_kernel::registry::Capability;
use sx_kernel::types::{CALL_ERROR_KIND_NAME, Type, show_definition, show_type};

#[derive(Default)]
struct Named {
    enums: Vec<(String, Json)>,
    types: Vec<(String, Json)>,
}

impl Named {
    fn holds(&self, name: &str) -> bool {
        self.enums.iter().chain(&self.types).any(|(held, _)| held == name)
    }

    fn visit(&mut self, ty: &Type) {
        match ty {
            Type::Primitive(_) | Type::Variable(_) | Type::Never => {}
            Type::List(inner) | Type::Map(inner) | Type::Option(inner) => self.visit(inner),
            Type::Tuple(items) => items.iter().for_each(|item| self.visit(item)),
            Type::Result(ok, err) => {
                self.visit(ok);
                self.visit(err);
            }
            Type::Record(record) => {
                if let Some(name) = &record.name
                    && !self.holds(name)
                {
                    self.types.push((name.clone(), Json::string(show_definition(ty))));
                }
                record.fields.iter().for_each(|(_, field)| self.visit(field));
            }
            Type::Enum(found) => {
                if found.name != CALL_ERROR_KIND_NAME && !self.holds(&found.name) {
                    let variants = found
                        .variants
                        .iter()
                        .map(|(variant, payload)| (variant.clone(), payload.as_ref().map_or(Json::Null, |p| Json::string(show_type(p)))))
                        .collect();
                    self.enums.push((found.name.clone(), Json::Object(variants)));
                }
                found.variants.iter().filter_map(|(_, payload)| payload.as_ref()).for_each(|payload| self.visit(payload));
            }
            Type::Nominal(found) => {
                if !self.holds(&found.name) {
                    self.types.push((found.name.clone(), Json::string(show_type(&found.base))));
                }
                self.visit(&found.base);
            }
        }
    }
}

pub fn describe<'c>(capabilities: impl IntoIterator<Item = &'c Capability>) -> Json {
    let mut named = Named::default();
    let mut listed = Vec::new();
    for capability in capabilities {
        named.visit(&capability.input);
        named.visit(&capability.output);
        listed.push(Json::object([
            ("id", Json::string(capability.id.as_str())),
            ("effect", Json::string(capability.effect.name())),
            ("idempotent", Json::Bool(capability.idempotent)),
            ("description", Json::string(capability.description.as_str())),
            ("input", Json::string(show_type(&capability.input))),
            ("output", Json::string(show_type(&capability.output))),
        ]));
    }
    Json::object([
        ("enums", Json::Object(named.enums)),
        ("types", Json::Object(named.types)),
        ("capabilities", Json::Array(listed)),
    ])
}
