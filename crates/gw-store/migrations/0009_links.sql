-- The graph is this table. One row per ordered pair: a page linking to another twice is
-- one edge, because that is what a graph draws and what a backlinks panel lists.
--
-- Both sides CASCADE. A link is a fact about two documents and outlives neither: deleting
-- a page must not leave an edge pointing at nothing, which would be a node in the graph
-- with a title nobody can read.
CREATE TABLE links (
    from_doc TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    to_doc   TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    PRIMARY KEY (from_doc, to_doc)
) WITHOUT ROWID;

CREATE INDEX links_to_doc ON links(to_doc);
