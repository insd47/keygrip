use keygrip::Schema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Schema)]
#[entity(sk(id))]
struct Record {
    id: String,
}

fn main() {}
