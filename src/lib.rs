//! A simple and fast (probably) domain pattern matcher
//!
//! A quick example of how and what it matches
//!
//! &nbsp;|`domain.tld`|`sub.domain.tld`|`sub.sub.domain.tld`
//! --|--|--|--|
//! `domain.tld`|✅|🅾️|🅾️
//! `*.domain.tld`|✅|✅|🅾️
//! `+.domain.tld`|🅾️|✅|🅾️
//! `**.domain.tld`|✅|✅|✅
//! `**+.domain.tld`|🅾️|✅|✅
//!
//! # Implementation notes
//!
//! There's some form of algorithmic blow up when doing `*.*.*.*.*.*`, this could be worked out in future versions, TODO etc
//!
//!

use std::borrow::Cow;
use std::fmt::{Display, Formatter};
use std::mem;
#[cfg(feature = "smallvec")]
use smallvec::SmallVec;

#[cfg(not(feature = "smallvec"))]
type StepVec<'a> = Vec<DomainPatternPart<'a>>;

#[cfg(not(feature = "smallvec"))]
type StackVec = Vec<usize>;

#[cfg(feature = "smallvec")]
type StepVec<'a> = SmallVec<[DomainPatternPart<'a>; 24]>;

#[cfg(feature = "smallvec")]
type StackVec = SmallVec<[usize; 32]>;

#[derive(Clone, Eq, PartialEq, Debug)]
pub struct DomainPattern<'a, const SPLITTER: char = '.'> {
    steps: StepVec<'a>,
}

impl<'a, const SPLITTER: char> DomainPattern<'a, SPLITTER> {
    pub fn parse(pattern: &'a str) -> Result<Self, InvalidToken<'a>> {
        pattern.try_into()
    }

    pub fn to_owned(&self) -> DomainPattern<'static> {
        DomainPattern {
            steps: self.steps.iter().map(|e| match e {
                DomainPatternPart::Static(s) => DomainPatternPart::Static(Cow::Owned(s.as_ref().to_owned())),
                DomainPatternPart::Wildcard(w) => DomainPatternPart::Wildcard(*w),
            }).collect(),
        }
    }

    pub fn matches(&self, domain: &str) -> bool {
        let mut stack: StackVec = Default::default();
        let mut next_stack: StackVec = Default::default();

        stack.push(0);

        let mut saw_last = false;

        for label in domain.split(SPLITTER) {
            if label == "" {
                continue;
            }

            saw_last = false;
            stack.sort();

            let mut last_path = None;

            for path in &stack {
                if *path >= self.steps.len() {
                    continue;
                }

                if Some(path) == last_path {
                    continue;
                }

                last_path = Some(path);

                let part = &self.steps[*path];
                match part {
                    DomainPatternPart::Static(d) => {
                        if d != label {
                            continue;
                        }
                    }
                    DomainPatternPart::Wildcard(w) => {
                        if w.multi {
                            next_stack.push(*path);
                        }
                    }
                }


                let mut next_idx = path + 1;

                if next_idx == self.steps.len() {
                    saw_last |= true;
                    continue;
                }

                next_stack.push(next_idx);

                while let DomainPatternPart::Wildcard(DomainPatternWildcard { optional: true, .. }) = &self.steps[next_idx] {
                    let jump_idx = next_idx + 1;
                    if jump_idx == self.steps.len() {
                        saw_last |= true;
                        break;
                    }

                    next_stack.push(jump_idx);
                    next_idx = jump_idx;
                }
            }

            mem::swap(&mut stack, &mut next_stack);
            next_stack.truncate(0);
        }

        saw_last
    }
}

impl<'a, const SPLITTER: char> TryFrom<&'a str> for DomainPattern<'a, SPLITTER> {
    type Error = InvalidToken<'a>;

    fn try_from(s: &'a str) -> Result<Self, Self::Error> {
        let mut steps: StepVec = Default::default();
        let mut offset = 0;
        for part in s.split(SPLITTER) {
            let position = offset;
            offset += part.len() + 1;

            let (optional, mut multi) = match part {
                "*" => (true, false),
                "+" => (false, false),
                "**" => (true, true),
                "**+" => (false, true),
                x if x.contains(|e| e == '*' || e == '+') => {
                    return Err(InvalidToken {
                        position,
                        unexpected_token: Cow::Borrowed(x),
                        full_string: Cow::Borrowed(s),
                    });
                }

                _ => {
                    steps.push(DomainPatternPart::Static(Cow::Borrowed(part)));
                    offset += 1 + part.len();
                    continue;
                }
            };

            // "optimizer"
            // folds parts together, or changes the previous for better performance
            if let Some(DomainPatternPart::Wildcard(DomainPatternWildcard { multi: last_multi, optional: last_optional })) = steps.last_mut() {
                // **.** = **
                if *last_multi && *last_optional && multi && optional {
                    continue;
                }

                // **.+ = **+
                // **+.* = **+
                // +.* != **.+
                if optional != *last_optional && (*last_multi || multi) {
                    *last_multi = true;
                    *last_optional = false;
                    continue;
                }

                // this should limit the amount of forking needed
                // **+.**+ = +.**+
                // Too make sure it keeps cascading, it'll also apply:
                // **+.+ = +.**+
                if *last_multi && !optional && !*last_optional {
                    *last_multi = false;
                    multi = true;
                }
            }

            steps.push(DomainPatternPart::Wildcard(DomainPatternWildcard {
                multi,
                optional,
            }));

            offset += 1 + part.len();
        }

        Ok(DomainPattern {
            steps
        })
    }
}

#[derive(Debug)]
pub struct InvalidToken<'a> {
    position: usize,
    unexpected_token: Cow<'a, str>,
    full_string: Cow<'a, str>,
}

impl Display for InvalidToken<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Invalid token {:?} at position {} in pattern  {:?}", self.unexpected_token, self.position, self.full_string)
    }
}

impl std::error::Error for InvalidToken<'_> {}

#[derive(Clone, Eq, PartialEq, Debug)]
pub enum DomainPatternPart<'a> {
    Static(Cow<'a, str>),
    Wildcard(DomainPatternWildcard),
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct DomainPatternWildcard {
    multi: bool,
    optional: bool,
}

#[cfg(test)]
mod tests {
    use crate::{DomainPattern, DomainPatternWildcard, DomainPatternPart};

    #[test]
    pub fn test_algorithmic_blowup() {
        let pattern: DomainPattern = "*.*.*.*.*.*.*.*.*.nice".try_into().expect("failed to parse");

        assert!(pattern.matches("nice.nice.nice.nice.nice.nice.nice.nice.nice.nice"))
    }

    #[test]
    pub fn test_simple() {
        let pattern: DomainPattern = "nice.**.nice".try_into().expect("failed to parse");
        assert!(pattern.matches("nice.nice.nice.nice"));
        assert!(pattern.matches("nice.nice.nice"));
        assert!(pattern.matches("nice.nice"));
        assert!(!pattern.matches("nice"));

        let pattern: DomainPattern = "nice.**+.nice".try_into().expect("failed to parse");
        assert!(pattern.matches("nice.nice.nice.nice"));
        assert!(pattern.matches("nice.nice.nice"));
        assert!(!pattern.matches("nice.nice"));
        assert!(!pattern.matches("nice"));

        let pattern: DomainPattern = "nice.*.nice".try_into().expect("failed to parse");
        assert!(!pattern.matches("nice.nice.nice.nice"));
        assert!(pattern.matches("nice.nice.nice"));
        assert!(pattern.matches("nice.nice"));
        assert!(!pattern.matches("nice"));

        let pattern: DomainPattern = "nice.+.nice".try_into().expect("failed to parse");
        assert!(!pattern.matches("nice.nice.nice.nice"));
        assert!(pattern.matches("nice.nice.nice"));
        assert!(!pattern.matches("nice.nice"));
        assert!(!pattern.matches("nice"));

        let pattern: DomainPattern = "nice.nice".try_into().expect("failed to parse");
        assert!(!pattern.matches("nice.nice.nice.nice"));
        assert!(!pattern.matches("nice.nice.nice"));
        assert!(pattern.matches("nice.nice"));
        assert!(!pattern.matches("nice"));


        let pattern: DomainPattern = "+.nice.**".try_into().expect("failed to parse");
        assert!(pattern.matches("nice.nice.nice.nice"));
        assert!(pattern.matches("nice.nice.nice"));
        assert!(pattern.matches("nice.nice"));
        assert!(!pattern.matches("nice.wow"));
        assert!(!pattern.matches("nice"));

        let pattern: DomainPattern<'/'> = "+/nice/**".try_into().expect("failed to parse");
        assert!(pattern.matches("nice/nice/nice/nice"));
        assert!(pattern.matches("nice/nice/nice"));
        assert!(pattern.matches("nice/nice"));
        assert!(!pattern.matches("nice/wow"));
        assert!(!pattern.matches("nice"));

        let pattern: DomainPattern = "x.**.**".try_into().expect("failed to parse");
        assert!(pattern.matches("x"));
        assert!(pattern.matches("x.x"));
        assert!(pattern.matches("x.x.x"));
        assert!(pattern.matches("x.x.x.x"));
        assert!(!pattern.matches("y"));

        let pattern: DomainPattern = "x.*.*".try_into().expect("failed to parse");
        assert!(pattern.matches("x"));
        assert!(pattern.matches("x.x"));
        assert!(pattern.matches("x.x.x"));
        assert!(!pattern.matches("x.x.x.x"));
        assert!(!pattern.matches("y"));

        let pattern: DomainPattern = "x.*.**".try_into().expect("failed to parse");
        assert!(pattern.matches("x"));
        assert!(pattern.matches("x.x"));
        assert!(pattern.matches("x.x.x"));
        assert!(pattern.matches("x.x.x.x"));
        assert!(!pattern.matches("y"));

        let pattern: DomainPattern = "**.**.**.**.**".try_into().expect("failed to parse");
        assert_eq!(pattern.steps.len(), 1);

        assert!(pattern.matches("x"));
        assert!(pattern.matches("x.x"));
        assert!(pattern.matches("x.x.x"));
        assert!(pattern.matches("x.x.x.x"));

        let pattern: DomainPattern = "**+.+.**+".try_into().expect("failed to parse");
        assert!(pattern.steps[..2].iter().all(|e| matches!(e, DomainPatternPart::Wildcard(DomainPatternWildcard { multi: false, optional: false }))));
    }
}
