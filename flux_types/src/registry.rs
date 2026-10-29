use std::collections::HashMap;

use crate::Type;

#[derive(Debug, Default, Clone)]
pub struct TypeRegistry {
    pub structs: HashMap<String, Type>,
    pub enums: HashMap<String, Type>,
    pub variant_to_enum: HashMap<String, String>,
}

impl TypeRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_struct(&mut self, name: String, fields: Vec<(String, Type)>) {
        self.structs
            .insert(name.clone(), Type::Struct { name, fields });
    }

    pub fn register_enum(&mut self, name: String, variants: Vec<(String, Option<Type>)>) {
        for (v, _) in &variants {
            self.variant_to_enum.insert(v.clone(), name.clone());
        }
        self.enums
            .insert(name.clone(), Type::Enum { name, variants });
    }

    pub fn resolve_name(&self, name: &str) -> Option<Type> {
        self.structs
            .get(name)
            .cloned()
            .or_else(|| self.enums.get(name).cloned())
    }

    pub fn enum_for_variant(&self, variant: &str) -> Option<&str> {
        self.variant_to_enum.get(variant).map(|s| s.as_str())
    }
}
