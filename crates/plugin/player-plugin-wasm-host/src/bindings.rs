macro_rules! with_vesper_plugin_wit {
    ($consumer:ident $(, $args:tt)*) => {
        $consumer! {
            r#"package vesper:plugin;

interface protocol {
  enum diagnostic-severity {
    info,
    warning,
    error,
  }

  record attribute {
    key: string,
    value: string,
  }

  record diagnostic {
    code: string,
    severity: diagnostic-severity,
    message: string,
    attributes: list<attribute>,
  }

  record measurement {
    name: string,
    value: f64,
    unit: string,
    attributes: list<attribute>,
  }

  record pipeline-event {
    run-id: string,
    session-id: string,
    platform: string,
    protocol: option<string>,
    event-name: string,
    timestamp-ns: u64,
    thread: option<string>,
    resource-identity: option<string>,
    attributes: list<attribute>,
    diagnostic: option<diagnostic>,
  }

  record event-hook-outcome {
    accepted: bool,
    measurements: list<measurement>,
    diagnostics: list<diagnostic>,
  }

  record benchmark-event {
    run-id: string,
    session-id: string,
    platform: string,
    protocol: option<string>,
    event-name: string,
    timestamp-ns: u64,
    elapsed-ns: u64,
    thread: option<string>,
    attributes: list<attribute>,
  }

  record benchmark-batch {
    events: list<benchmark-event>,
  }

  record threshold-violation {
    measurement: string,
    actual: f64,
    threshold: f64,
    comparison: string,
  }

  record benchmark-report {
    accepted-events: u64,
    dropped-events: u64,
    measurements: list<measurement>,
    threshold-violations: list<threshold-violation>,
    diagnostics: list<diagnostic>,
  }

  variant plugin-error {
    invalid-input(string),
    rejected(string),
    failed(string),
  }
}

interface host {
  enum log-level {
    trace,
    debug,
    info,
    warn,
    error,
  }

  log: func(level: log-level, code: string, message: string);
}

interface event-hook {
  use protocol.{pipeline-event, event-hook-outcome, plugin-error};
  on-event: func(event: pipeline-event) -> result<event-hook-outcome, plugin-error>;
}

interface benchmark-sink {
  use protocol.{benchmark-batch, benchmark-report, plugin-error};
  on-event-batch: func(batch: benchmark-batch) -> result<u64, plugin-error>;
  flush: func() -> result<benchmark-report, plugin-error>;
}

world event-hook-plugin {
  import host;
  export event-hook;
}

world benchmark-sink-plugin {
  import host;
  export benchmark-sink;
}

world event-and-benchmark-plugin {
  import host;
  export event-hook;
  export benchmark-sink;
}
"#
            $(, $args)*
        }
    };
}

macro_rules! define_wit_constant {
    ($wit:literal) => {
        pub const VESPER_PLUGIN_WIT: &str = $wit;
    };
}

macro_rules! generate_bindings {
    ($wit:literal, $world:literal) => {
        wasmtime::component::bindgen!({
            inline: $wit,
            world: $world,
            imports: {
                "vesper:plugin/host.log": trappable,
            },
        });
    };
}

with_vesper_plugin_wit!(define_wit_constant);

pub(crate) mod event_hook {
    #![allow(dead_code, unsafe_code, unused_variables)]

    with_vesper_plugin_wit!(generate_bindings, "event-hook-plugin");
}

pub(crate) mod benchmark_sink {
    #![allow(dead_code, unsafe_code, unused_variables)]

    with_vesper_plugin_wit!(generate_bindings, "benchmark-sink-plugin");
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use super::VESPER_PLUGIN_WIT;

    #[test]
    fn embedded_wit_matches_repository_source() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .join("wit/vesper-plugin/plugin.wit");
        if !path.is_file() {
            return;
        }

        let canonical = fs::read_to_string(path).expect("read canonical Vesper plugin WIT");
        assert_eq!(VESPER_PLUGIN_WIT, canonical);
    }
}
