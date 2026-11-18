#[cfg(feature = "config")]
use moldette_config::Specs;
use moldette_core::{Explain, Malformed, Response, StatusCounts};
use moldette_mapper::MapperSpec;
use moldette_parser::ParserSpec;
use moldette_resp::RespSpec;

/// A parser, mapper and resp spec run in turn on each raw record. Immutable
/// once built; callers share it behind an `Arc` and replace that `Arc` to
/// apply new specs (#8).
#[derive(Debug, Clone)]
pub struct Pipeline {
    parser: ParserSpec,
    mapper: MapperSpec,
    resp: RespSpec,
}

impl Pipeline {
    /// Runs the given specs in turn. Cross-layer checks, such as the mapper
    /// and resp sharing one schema, belong to `moldette-config` and are not
    /// repeated here (#3).
    pub fn new(parser: ParserSpec, mapper: MapperSpec, resp: RespSpec) -> Self {
        Self {
            parser,
            mapper,
            resp,
        }
    }

    /// Copies the parser, mapper and resp specs of `specs`, which have passed
    /// the cross-layer checks of `Config::build`. Returns `None` when one of
    /// the three sections is missing.
    #[cfg(feature = "config")]
    pub fn from_specs(specs: &Specs) -> Option<Self> {
        Some(Self::new(
            specs.parser()?.clone(),
            specs.mapper()?.clone(),
            specs.resp()?.clone(),
        ))
    }

    /// Parses, maps and responds to one raw record. A record the parser cannot
    /// read gives [`Response::Malformed`].
    pub fn process<'a>(&'a self, raw: &'a str) -> Response<'a> {
        match self.parser.parse(raw) {
            Ok(record) => self.resp.respond(self.mapper.map(&record)),
            Err(malformed) => Response::Malformed(malformed),
        }
    }

    /// Processes each raw record in input order, as [`process`](Self::process)
    /// does, and counts the responses per status, malformed records included.
    pub fn process_batch<'a, I>(&'a self, raws: I) -> (Vec<Response<'a>>, StatusCounts)
    where
        I: IntoIterator<Item = &'a str>,
    {
        let mut malformed: Vec<(usize, Malformed)> = Vec::new();
        let mapped =
            raws.into_iter()
                .enumerate()
                .filter_map(|(index, raw)| match self.parser.parse(raw) {
                    Ok(record) => Some(self.mapper.map(&record)),
                    Err(reason) => {
                        malformed.push((index, reason));
                        None
                    }
                });
        let (responded, _) = self.resp.respond_batch(mapped);
        let mut responses = Vec::with_capacity(responded.len() + malformed.len());
        let mut responded = responded.into_iter();
        for (index, reason) in malformed {
            responses.extend(responded.by_ref().take(index - responses.len()));
            responses.push(Response::Malformed(reason));
        }
        responses.extend(responded);
        let counts = responses.iter().collect();
        (responses, counts)
    }

    /// Processes one raw record as [`process`](Self::process) does and
    /// records how the mapper filled each field (#39). The record is `None`
    /// for a malformed record, which never reaches the mapper.
    pub fn explain<'a>(&'a self, raw: &'a str) -> (Response<'a>, Option<Explain<'a>>) {
        match self.parser.parse(raw) {
            Ok(record) => {
                let (mapped, explain) = self.mapper.explain(&record);
                (self.resp.respond(mapped), Some(explain))
            }
            Err(malformed) => (Response::Malformed(malformed), None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn public_types_are_send_and_sync() {
        assert_send_sync::<Pipeline>();
    }
}
