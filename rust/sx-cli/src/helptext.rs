pub const TOP_HELP: &str = r#"usage: sx [-h] {help,cores,check,explain,run} ...

Check, explain, and run Sx programs.

positional arguments:
  {help,cores,check,explain,run}
    help                Print the guide, or the entry for one name.
    cores               List the registered cores and the evidence for each.
    check               Check a program and print its diagnostics.
    explain             Print the tools, effects, inputs, and result type of a
                        program.
    run                 Run a program against recorded tool responses.

options:
  -h, --help            show this help message and exit
"#;

pub const HELP_HELP: &str = r#"usage: sx help [-h] [--declarations DECLARATIONS]
               [--level {minimal,standard,full}]
               [name]

positional arguments:
  name

options:
  -h, --help            show this help message and exit
  --declarations DECLARATIONS
                        JSON declarations that add tools and types
  --level {minimal,standard,full}
                        Guide detail
"#;

pub const CORES_HELP: &str = r#"usage: sx cores [-h] [--format {text,json}]

options:
  -h, --help            show this help message and exit
  --format {text,json}
"#;

pub const CHECK_HELP: &str = r#"usage: sx check [-h] [--declarations DECLARATIONS] [--allow ID]
                [--format {text,json}] [--emit {ir}]
                file

positional arguments:
  file

options:
  -h, --help            show this help message and exit
  --declarations DECLARATIONS
                        JSON declarations of tools, types, and inputs
  --allow ID            Authorize one tool
  --format {text,json}
  --emit {ir}           Print the checked semantic IR as JSON
"#;

pub const EXPLAIN_HELP: &str = r#"usage: sx explain [-h] [--declarations DECLARATIONS] [--allow ID]
                  [--format {text,json}]
                  file

positional arguments:
  file

options:
  -h, --help            show this help message and exit
  --declarations DECLARATIONS
                        JSON declarations of tools, types, and inputs
  --allow ID            Authorize one tool
  --format {text,json}
"#;

pub const RUN_HELP: &str = r#"usage: sx run [-h] [--declarations DECLARATIONS] [--allow ID]
              [--format {text,json}] [--responses RESPONSES]
              [--input NAME=JSON]
              file

positional arguments:
  file

options:
  -h, --help            show this help message and exit
  --declarations DECLARATIONS
                        JSON declarations of tools, types, and inputs
  --allow ID            Authorize one tool
  --format {text,json}
  --responses RESPONSES
                        JSON list of recorded tool responses
  --input NAME=JSON     One input value
"#;

