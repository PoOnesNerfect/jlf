use crate::Json;

/// A single `key OP value[,value...]` predicate over a parsed record.
#[derive(Debug, Clone)]
pub struct Filter {
    path: Vec<String>,
    op: Op,
    values: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Op {
    Eq,
    Ne,
    Gt,
    Lt,
    Ge,
    Le,
    Contains,
    NotContains,
}

impl Filter {
    /// Parse a token like `level=error`, `latency>500`, `msg~timeout`,
    /// `level=error,warn`. Returns `None` if the token has no operator.
    pub fn parse(token: &str) -> Option<Filter> {
        // order matters: check 2-char ops before 1-char
        let ops: &[(&str, Op)] = &[
            ("!=", Op::Ne),
            (">=", Op::Ge),
            ("<=", Op::Le),
            ("!~", Op::NotContains),
            ("~", Op::Contains),
            ("=", Op::Eq),
            (">", Op::Gt),
            ("<", Op::Lt),
        ];
        for (sym, op) in ops {
            if let Some(i) = token.find(sym) {
                let key = &token[..i];
                let val = &token[i + sym.len()..];
                if key.is_empty() {
                    return None;
                }
                return Some(Filter {
                    path: key.split('.').map(str::to_owned).collect(),
                    op: *op,
                    values: val.split(',').map(str::to_owned).collect(),
                });
            }
        }
        None
    }

    pub fn matches(&self, json: &Json) -> bool {
        let mut cur = json;
        for seg in &self.path {
            cur = match seg.parse::<usize>() {
                Ok(i) => cur.get_i(i),
                Err(_) => cur.get(seg),
            };
        }
        let field = cur.as_str().or_else(|| cur.as_value());
        let Some(field) = field else {
            // missing field: only !=/!~ can match
            return matches!(self.op, Op::Ne | Op::NotContains);
        };
        match self.op {
            Op::Eq => self.values.iter().any(|v| v == field),
            Op::Ne => self.values.iter().all(|v| v != field),
            Op::Contains => self.values.iter().any(|v| field.contains(v.as_str())),
            Op::NotContains => self.values.iter().all(|v| !field.contains(v.as_str())),
            Op::Gt | Op::Lt | Op::Ge | Op::Le => {
                let Ok(f) = field.parse::<f64>() else { return false };
                self.values.iter().any(|v| {
                    v.parse::<f64>().is_ok_and(|n| match self.op {
                        Op::Gt => f > n,
                        Op::Lt => f < n,
                        Op::Ge => f >= n,
                        Op::Le => f <= n,
                        _ => false,
                    })
                })
            }
        }
    }
}

/// All filters must match (AND).
pub fn matches_all(filters: &[Filter], json: &Json) -> bool {
    filters.iter().all(|f| f.matches(json))
}
