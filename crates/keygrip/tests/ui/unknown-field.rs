use keygrip::Schema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Schema)]
#[entity(pk(user_id))]
struct Record {
    id: String,
}

fn main() {}
