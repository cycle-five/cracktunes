use lazy_static::lazy_static;
//use prometheus::{labels, opts, register_counter, register_gauge, register_histogram_vec};
use prometheus::{CounterVec, Gauge, HistogramOpts, HistogramVec, Opts, Registry};

lazy_static! {
    pub static ref REGISTRY: Registry = Registry::new();
    //.cb=onst_label("command", "admin")
    pub static ref COMMAND_EXECUTIONS: CounterVec = CounterVec::new(
        Opts::new("command_executions", "Command Executions"),
        &["admin",
          "autopause",
          "chatgpt",
          "clear",
          "deafen",
          "authorize",
          "deauthorize",
          "help",
          "join",
          "kick",
          "leave",
          "lyrics",
          "mute",
          "pause",
          "play",
          "queue",
          "resume",
          "skip",
          "stop",
          "shuffle",
          "summon",
          "version",
          "unmute",
          "volume",
          "voteskip"]
    )
    .expect("metric can be created");
    pub static ref CONNECTED_CLIENTS: Gauge =
        Gauge::new("connected_clients", "Connected Clients").expect("metric can be created");
    pub static ref RESPONSE_CODE_COLLECTOR: CounterVec = CounterVec::new(
        Opts::new("response_code", "Response Codes"),
        &["env", "statuscode", "type"]
    )
    .expect("metric can be created");
    pub static ref RESPONSE_TIME_COLLECTOR: HistogramVec = HistogramVec::new(
        HistogramOpts::new("response_time", "Response Times"),
        &["env"]
    )
    .expect("metric can be created");
}
