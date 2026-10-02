use keygrip::Schema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Schema)]
#[entity(pk())]
struct Record {
    id: String,
}

fn main() {}
