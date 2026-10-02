use keygrip::Schema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Schema)]
struct Record {
    id: String,
}

fn main() {}
