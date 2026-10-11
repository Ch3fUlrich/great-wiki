//! The dependency graph between a dataset's formula fields: which formula reads which, and
//! whether those reads close into a cycle. Iterative on purpose — a dataset may hold a hundred
//! formula columns and a recursive walk would grow the call stack with them, so this walks an
//! explicit stack instead and never recurses.

use std::collections::HashMap;

/// One node and the keys it reads. A dependency naming no node of the graph is ignored: a
/// formula may read a stored column, or a field that is not a formula at all, and neither is
/// a cycle.
///
/// Returns the keys along one cycle, the head repeated at the end so the caller can render
/// `a -> b -> a`; a self-reference is `a -> a`. `None` when the graph is acyclic.
pub fn find_cycle(graph: &[(String, Vec<String>)]) -> Option<Vec<String>> {
    // First occurrence of a key wins as its node index; dataset keys are unique anyway.
    let mut index: HashMap<&str, usize> = HashMap::with_capacity(graph.len());
    for (i, (key, _)) in graph.iter().enumerate() {
        index.entry(key.as_str()).or_insert(i);
    }

    // 0 = unseen, 1 = on the current path, 2 = finished.
    let mut state = vec![0u8; graph.len()];
    for start in 0..graph.len() {
        if state[start] != 0 {
            continue;
        }
        // (node, next child to inspect) and the path of nodes from `start` down to `node`.
        let mut stack = vec![(start, 0usize)];
        let mut path = vec![start];
        state[start] = 1;
        while !stack.is_empty() {
            let top = stack.len() - 1;
            let (node, child) = stack[top];
            if child < graph[node].1.len() {
                stack[top].1 = child + 1;
                // Only a dependency that is itself a node can close a cycle.
                let Some(&next) = index.get(graph[node].1[child].as_str()) else {
                    continue;
                };
                match state[next] {
                    1 => {
                        let at = path.iter().position(|&p| p == next)?;
                        let mut cycle: Vec<String> =
                            path[at..].iter().map(|&i| graph[i].0.clone()).collect();
                        cycle.push(graph[next].0.clone());
                        return Some(cycle);
                    }
                    0 => {
                        state[next] = 1;
                        path.push(next);
                        stack.push((next, 0));
                    }
                    _ => {}
                }
            } else {
                stack.pop();
                state[node] = 2;
                path.pop();
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::find_cycle;

    fn graph(pairs: &[(&str, &[&str])]) -> Vec<(String, Vec<String>)> {
        pairs
            .iter()
            .map(|(k, deps)| (k.to_string(), deps.iter().map(|d| d.to_string()).collect()))
            .collect()
    }

    fn keys(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_formula_that_reads_itself_is_a_cycle() {
        assert_eq!(
            find_cycle(&graph(&[("a", &["a"])])),
            Some(keys(&["a", "a"]))
        );
    }

    #[test]
    fn two_formulas_reading_each_other_are_a_cycle() {
        assert_eq!(
            find_cycle(&graph(&[("a", &["b"]), ("b", &["a"])])),
            Some(keys(&["a", "b", "a"]))
        );
    }

    #[test]
    fn a_three_formula_loop_is_found() {
        assert_eq!(
            find_cycle(&graph(&[("a", &["b"]), ("b", &["c"]), ("c", &["a"])])),
            Some(keys(&["a", "b", "c", "a"]))
        );
    }

    #[test]
    fn a_shared_dependency_is_not_a_cycle() {
        // a -> b, a -> c, b -> d, c -> d: a diamond, every path runs source to sink.
        let g = graph(&[("a", &["b", "c"]), ("b", &["d"]), ("c", &["d"]), ("d", &[])]);
        assert_eq!(find_cycle(&g), None);
    }

    #[test]
    fn a_dependency_on_a_field_that_is_not_a_node_is_ignored() {
        // "stored" is not a formula, so a reading it is not a cycle even though two formulas
        // read the same stored column.
        let g = graph(&[("a", &["stored"]), ("b", &["stored", "a"])]);
        assert_eq!(find_cycle(&g), None);
    }
}
