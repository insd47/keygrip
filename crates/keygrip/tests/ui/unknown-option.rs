use keygrip::Schema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Schema)]
#[entity(pk(id), table = "Records")]
struct Record {
    id: String,
}

fn main() {}
