use keygrip::{Index, KeyPart, Parts, Schema, SortSpace};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
enum Kind {
    Submit,
    Test,
}

impl KeyPart for Kind {
    fn part(&self) -> String {
        match self {
            Self::Submit => "S",
            Self::Test => "T",
        }
        .into()
    }
}

#[derive(Serialize, Deserialize, Schema)]
#[entity(pk(user_id), sk(problem_id), index(name = "byProblem", pk(problem_id), sk(user_id)))]
struct SubmissionTable {
    user_id: String,
    problem_id: String,
}

#[derive(Serialize, Deserialize, Schema)]
#[entity(name = "Execution", pk(user_id), sk(problem_id, kind, id))]
struct ExecutionTable {
    user_id: String,
    problem_id: String,
    kind: Kind,
    id: String,
}

#[derive(Serialize, Deserialize, Schema)]
#[entity(pk(user_id), sk("run", problem_id, id))]
struct RunTable {
    user_id: String,
    problem_id: String,
    id: String,
}

#[derive(Serialize, Deserialize, Schema)]
#[entity(pk(user_id), sk("gate"))]
struct GateTable {
    user_id: String,
    rev: i64,
}

#[test]
fn single_field_keys_use_the_camel_case_field_name() {
    assert_eq!(SubmissionTable::NAME, "Submission");
    assert_eq!(SubmissionTable::SPACE, None);
    assert_eq!(
        SubmissionTable::parts(("user", "problem")),
        Parts::two("userId", "user", "problemId", "problem")
    );
}

#[test]
fn composite_keys_join_encoded_parts_under_pk_and_sk() {
    let execution = ExecutionTable {
        user_id: "user".into(),
        problem_id: "problem".into(),
        kind: Kind::Submit,
        id: "id".into(),
    };

    assert_eq!(ExecutionTable::NAME, "Execution");
    assert_eq!(ExecutionTable::SPACE, None);
    assert_eq!(
        ExecutionTable::parts(execution.primary()),
        Parts::two("pk", "user", "sk", "problem#S#id")
    );
    assert_eq!(ExecutionTable::prefix("problem", &Kind::Test), "problem#T#");
}

#[test]
fn leading_sort_literals_open_a_prefix_space() {
    assert_eq!(RunTable::SPACE, Some(SortSpace::Prefix("run")));
    assert_eq!(
        RunTable::parts(("user", "problem", "id")),
        Parts::two("pk", "user", "sk", "run#problem#id")
    );
    assert_eq!(RunTable::prefix("problem"), "problem#");
}

#[test]
fn literal_only_sort_keys_open_an_exact_space() {
    let gate = GateTable {
        user_id: "user".into(),
        rev: 0,
    };

    assert_eq!(GateTable::SPACE, Some(SortSpace::Exact("gate")));
    assert_eq!(GateTable::parts("user"), Parts::two("pk", "user", "sk", "gate"));
    assert_eq!(GateTable::parts(gate.primary()), GateTable::parts("user"));
}

#[test]
fn indexes_emit_constants_with_their_attribute_names() {
    assert_eq!(
        SubmissionTable::BY_PROBLEM,
        Index {
            name: "byProblem",
            partition: "problemId",
            sort: Some("userId"),
        }
    );
}
