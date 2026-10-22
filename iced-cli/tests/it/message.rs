#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Tick(pub u64);

impl rawr_import::profile::message::Message for Tick {
    fn encode(
        &self,
        output: &mut rawr_import::protocol::Writer<'_>,
    ) -> Result<(), rawr_import::profile::message::Error> {
        output.u64(self.0)
    }

    fn decode(
        input: &mut rawr_import::protocol::Reader<'_>,
    ) -> Result<Self, rawr_import::profile::message::Error> {
        input.u64().map(Self)
    }
}
