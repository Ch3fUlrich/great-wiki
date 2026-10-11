-- Relation fields (ADR 0029 cl. 2): a `relation` column points at another dataset's rows. The
-- row's `values` JSON keeps the ids somebody wrote, and this table keeps the same edges in a form
-- SQLite can answer two questions about at query time — whether a target row still exists (a delete
-- here cascades the edge away, so a dangling id is dropped at read rather than shown), and which
-- rows point AT a given row (the reverse lookup, served by `dataset_relation_by_target`).
--
-- `row_id` and `target_row_id` are `dataset_row` ids and `field_id` a `dataset_field` id, all
-- TEXT PRIMARY KEYs (0020), so the edges go with their row, their column and their target — a
-- cascade in three directions. WITHOUT ROWID because the primary key is the whole row.
CREATE TABLE dataset_relation (
    row_id        TEXT NOT NULL REFERENCES dataset_row(id) ON DELETE CASCADE,
    field_id      TEXT NOT NULL REFERENCES dataset_field(id) ON DELETE CASCADE,
    target_row_id TEXT NOT NULL REFERENCES dataset_row(id) ON DELETE CASCADE,
    PRIMARY KEY (row_id, field_id, target_row_id)
) WITHOUT ROWID;

CREATE INDEX dataset_relation_by_target ON dataset_relation (target_row_id, field_id);
