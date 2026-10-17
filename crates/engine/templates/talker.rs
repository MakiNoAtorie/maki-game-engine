//! ScriptName: on interact, lines of text and a question. Give the object the
//! components `"script_name": (...)` and `"area": ()`.

use bevy_ecs::component::Component;
use maki_engine::core::{App, Fields};
use maki_engine::events::{Builtin, Script, ScriptContext, add_script};
use serde::{Deserialize, Serialize};

#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize, Fields)]
#[serde(default, deny_unknown_fields)]
#[fields(default)]
pub struct ScriptName {
    /// The lines: one message for each part between `|`. Each part, the question,
    /// and each answer is a key of a text table (a key that no table has shows itself).
    pub lines: String,
    /// A question after the lines. Empty: none.
    pub question: String,
    /// The answers, between `|`.
    pub answers: String,
    /// The variable that gets the number of the answer that the player takes: 1
    /// for the first.
    pub answer: String,
}

impl Default for ScriptName {
    fn default() -> Self {
        Self {
            lines: "Hello.|It is cold here.".to_owned(),
            question: "Do you want the key?".to_owned(),
            answers: "Yes|No".to_owned(),
            answer: "script_name_answer".to_owned(),
        }
    }
}

impl Script for ScriptName {
    fn interact(&mut self, context: &mut ScriptContext) {
        // One list: each message waits for its press, then the next one shows.
        let lines = self.lines.split('|').filter(|line| !line.is_empty());
        let mut commands: Vec<_> = lines
            .filter_map(|line| context.command(Builtin::Message(line.into())))
            .collect();
        if !self.question.is_empty() {
            let answers = self.answers.split('|').enumerate().map(|(index, text)| {
                let set = Builtin::SetVariable(self.answer.clone(), index as i32 + 1);
                (text.into(), context.command(set).into_iter().collect())
            });
            let question = Builtin::Choice(self.question.as_str().into(), answers.collect());
            commands.extend(context.command(question));
        }
        context.run(commands);
    }
}

pub fn add(app: &mut App) {
    add_script::<ScriptName>(app, "script_name");
}
