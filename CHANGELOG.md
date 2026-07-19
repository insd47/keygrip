# Changelog

## 0.3.3

- Add `Entity::merge` for conditional whole-value updates with `if_not_exists`
  preservation and optional `ALL_NEW` results through `fetch`.
- Document that merge preserves attributes absent from the new value, unlike
  the full-replacement semantics of `store`.
