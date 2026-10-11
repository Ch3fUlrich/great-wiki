<!-- Fold into CHANGELOG.md under [Unreleased], in the section named below. -->

### Changed

- **There is now one definition of "a field this caller may not see", and every door uses it.**
  Before, a hidden relation and a hidden rollup were computed separately, in the schema path and in
  the query path, and formulas were not part of the decision at all — a formula that added one to a
  hidden rollup still came back with a value for a reader who could not see the rollup it read.
  The rule is now stated once, in `hidden_field_keys(principal, dataset)`, and it hides a field when

  - it is a `relation` whose target dataset the caller cannot read; or
  - it is a `rollup` over such a relation; or
  - it is a `formula` whose dependencies — followed transitively through other formulas and rollups,
    read off the field keys named by its stored `expr` — reach any field above.

  The formula step runs to a fixpoint, so `A = {B}`, `B = {hidden relation}` hides both; the loop is
  bounded by the column count regardless (a cycle is refused when the column is saved, so none is
  expected). A rollup whose stored config has gone bad — only reachable by writing under the API — is
  *not* treated as hidden: it is a hole that reads as no value for everyone, not a secret, so a
  formula over it still evaluates to an error cell rather than vanishing.

- **Every place that decides visibility now reads from that one set:** the schema (`dataset_fields`)
  omits hidden columns; a row never carries them, including a hidden formula's value, which is no
  longer computed at all; a filter, a sort, or any field a saved view names is refused in the exact
  words used for a key that does not exist — `no field \`x\`` — so asking about a hidden field and
  asking about a made-up one are indistinguishable (ADR 0022). Listing saved views strips hidden keys
  from each config instead of failing the whole view; creating or updating one refuses a config that
  names a hidden field, with that same unknown-field wording. The owner, who can read the target,
  sees every one of them and keeps the honest "…cannot be filtered or sorted yet" refusals.

### Known limitations

- A hidden formula is still *not computed* rather than computed-and-discarded, so a formula that
  reads both a hidden and a visible field is hidden whole. That is the intended fail-closed shape; a
  partial value would be the leak.
