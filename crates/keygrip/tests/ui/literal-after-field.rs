use keygrip::Schema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Schema)]
#[entity(pk(user_id), sk(id, "run"))]
struct Record {
    user_id: String,
    id: String,
}

fn main() {}
