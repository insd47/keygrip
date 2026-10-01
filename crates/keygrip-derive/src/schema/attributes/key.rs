use crate::schema::field::Field;
use crate::schema::utils;
use syn::LitStr;

#[derive(Default)]
pub struct Key {
    pub partition: Vec<Field>,
    /// Leading string literals of the sort key; they have no field to name
    /// an attribute after, so they force the synthetic `pk`/`sk` names.
    pub literals: Vec<LitStr>,
    pub sort: Vec<Field>,
}

impl Key {
    pub fn names(&self) -> Names {
        let composite = self.partition.len() > 1 || self.sort.len() > 1 || !self.literals.is_empty();
        let partition = if composite {
            "pk".into()
        } else {
            utils::attribute_name(&self.partition)
        };
        let sort = (!self.sort.is_empty() || !self.literals.is_empty()).then(|| {
            if composite {
                "sk".into()
            } else {
                utils::attribute_name(&self.sort)
            }
        });

        Names { partition, sort }
    }
}

pub struct Names {
    pub partition: String,
    pub sort: Option<String>,
}
