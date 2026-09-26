use super::invalid;
use crate::{Expression, Result, Schema};

const KEY: &str = "#keygripKey";

/// The condition that must hold for a write to apply.
///
/// The two existence checks resolve against the entity's own key schema, so
/// call sites never spell key attribute names; anything else is an
/// [`Expression`]:
///
/// ```
/// use keygrip::{Condition, Expression};
///
/// let absent = Condition::absent();
/// let fresh: Condition = Expression::new("revision = :revision")
///     .value(":revision", &0)
///     .into();
/// ```
#[derive(Debug, Clone)]
pub struct Condition(Kind);

#[derive(Debug, Clone)]
enum Kind {
    Absent,
    Exists,
    Expression(Expression),
}

impl Condition {
    /// Holds when no item exists at the write's primary key.
    pub fn absent() -> Self {
        Self(Kind::Absent)
    }

    /// Holds when an item exists at the write's primary key.
    pub fn exists() -> Self {
        Self(Kind::Exists)
    }

    fn resolve<E: Schema>(self) -> Expression {
        match self.0 {
            Kind::Absent => {
                Expression::new(format!("attribute_not_exists({KEY})")).name(KEY, E::PARTITION)
            }
            Kind::Exists => {
                Expression::new(format!("attribute_exists({KEY})")).name(KEY, E::PARTITION)
            }
            Kind::Expression(expression) => expression,
        }
    }
}

impl From<Expression> for Condition {
    fn from(expression: Expression) -> Self {
        Self(Kind::Expression(expression))
    }
}

/// A write's single condition slot; a second condition is recorded as a
/// problem and surfaces when the write compiles.
#[derive(Debug, Default)]
pub(crate) struct Slot {
    condition: Option<Condition>,
    problem: Option<String>,
}

impl Slot {
    pub(crate) fn attach(&mut self, condition: Condition) {
        if self.condition.is_some() {
            self.problem
                .get_or_insert_with(|| "a write cannot have more than one condition".into());
        } else {
            self.condition = Some(condition);
        }
    }

    pub(crate) fn resolve<E: Schema>(self) -> Result<Option<Expression>> {
        match self.problem {
            Some(problem) => Err(invalid(problem)),
            None => Ok(self.condition.map(Condition::resolve::<E>)),
        }
    }
}
