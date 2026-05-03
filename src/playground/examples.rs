pub(super) struct PlaygroundExample {
    pub relative_path_from_src: &'static str,
    pub source: &'static str,
}

pub(super) const PLAYGROUND_EXAMPLES: &[PlaygroundExample] = &[
    PlaygroundExample {
        relative_path_from_src: "Playground.par",
        source: include_str!("../../playground-examples/src/Playground.par"),
    },
    PlaygroundExample {
        relative_path_from_src: "PlaygroundChat.par",
        source: include_str!("../../playground-examples/src/PlaygroundChat.par"),
    },
    PlaygroundExample {
        relative_path_from_src: "RockPaperScissors.par",
        source: include_str!("../../playground-examples/src/RockPaperScissors.par"),
    },
    PlaygroundExample {
        relative_path_from_src: "Functional.par",
        source: include_str!("../../playground-examples/src/Functional.par"),
    },
    PlaygroundExample {
        relative_path_from_src: "JsonParsing.par",
        source: include_str!("../../playground-examples/src/JsonParsing.par"),
    },
];
