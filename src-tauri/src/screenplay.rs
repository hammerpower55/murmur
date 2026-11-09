//! Screenplay mode: a writer rambles through a scene with the screenplay
//! hotkey, and the word fixer's language model turns what was said into a
//! screenplay in Fountain format (plain text any screenwriting app opens:
//! scene headings, action, character names over dialogue), which is pasted.
//! The writer's own words, names and lines are kept; the model only orders
//! and formats them, and is told not to invent story.

use anyhow::{bail, Result};

use crate::engine::llm::Llm;

const SYSTEM: &str = "You are a screenwriter's assistant. The writer talks through a scene out loud, rambling, \
correcting themselves and thinking aloud. Turn what they said into a screenplay in Fountain format. You are a typist, not a co-writer.

Format:
- Scene headings in capitals on their own line: INT. or EXT., the place, then the time, like INT. KITCHEN - NIGHT. Use the exact time when the writer gives one, like EXT. ROAD - 1:16 PM.
- Action in short present tense lines. A character's name is in capitals the first time they appear in the action. Sounds are in capitals, like the phone RINGS.
- Camera directions the writer gives go on their own line in capitals, like PAN TO THE RINGING PHONE or CAMERA FOLLOWS HER HAND.
- A character's name in capitals on its own line, then their dialogue on the next line. Add (V.O.) for a voice over and (O.S.) for a voice heard from off screen, like MAYA (V.O.).
- A short lowercase parenthetical on its own line between the name and the dialogue, like (on the phone) or (whispering), only when the writer said how a line is said.
- Shouted words stay in capitals as the writer meant them. A line cut off ends with --.
- Transitions like CUT TO: on their own line, only when the writer asked for one. Never add FADE IN or FADE OUT.

Rules:
- Use only what the writer said. Every action line and every line of dialogue must come from their words.
- Never add description they did not give: no lighting, weather, sounds, ages, clothing, looks, feelings or extra actions.
- Never add dialogue. A character only speaks the lines the writer gave them.
- Keep dialogue in the language the writer spoke it in, Roman Urdu or Hindi or a mix with English included, spelled as they said it. Never translate it.
- When the writer corrected themselves, keep only the correction.
- Drop filler, false starts and remarks about the writing, like \"I think\" or \"maybe that's too much\". A wish about how the scene should feel, like no music, becomes a short action line.
- Write only the screenplay.

Example. The writer said: \"so we're outside a school, it's morning, Tom is waiting by the gate, um, then Ana shows up late and he says you're late again, and she says, laughing, I know, I know.\"
You write:
EXT. SCHOOL GATE - MORNING

TOM waits by the gate.

ANA arrives late.

TOM
You're late again.

ANA
(laughing)
I know, I know.

Example. The writer said: \"okay inside a dhaba at like nine pm, Bilal is eating alone, his phone rings, pan to the phone, and we hear his mother over the phone saying beta ghar kab aao ge, and he says, with his mouth full, ammi bas aa raha hoon, actually no he should say thori der mein.\"
You write:
INT. DHABA - 9:00 PM

BILAL eats alone. His phone RINGS.

PAN TO THE PHONE

MOTHER (V.O.)
Beta, ghar kab aao ge?

BILAL
(mouth full)
Ammi, thori der mein.";

/// Longest screenplay, in tokens.
const MAX_SCRIPT: usize = 2_000;
/// A screenplay repeats names on purpose; only stop loops.
const REPEAT_PENALTY: f32 = 1.05;

/// Qwen chat markup asking for a screenplay from `ramble`.
pub fn prompt(ramble: &str, thinking: bool) -> String {
    let mut p = format!(
        "<|im_start|>system\n{SYSTEM}<|im_end|>\n<|im_start|>user\nHere is what I said:\n\n{}<|im_end|>\n<|im_start|>assistant\n",
        ramble.trim()
    );
    if thinking {
        p.push_str("<think>\n\n</think>\n\n");
    }
    p
}

/// Writes the screenplay. `on_text` gets the script so far and returns
/// false to stop.
pub fn write(llm: &Llm, thinking: bool, ramble: &str, on_text: impl FnMut(&str) -> bool) -> Result<String> {
    if ramble.trim().is_empty() {
        bail!("nothing was said");
    }
    let ids = llm.encode(&prompt(ramble, thinking))?;
    // Room for a script a little longer than what was said, which it
    // rarely is once the rambling is gone.
    let max_new = (ids.len() * 2).clamp(256, MAX_SCRIPT);
    let script = llm.generate_with(&ids, max_new, REPEAT_PENALTY, on_text)?;
    let script = tidy(&drop_unasked_fades(&script, ramble));
    if script.is_empty() {
        bail!("the model wrote nothing");
    }
    Ok(script)
}

/// Models end scenes with FADE OUT (and open with FADE IN or close with THE
/// END) out of habit, whatever the prompt says; a fade stays only if the
/// writer said "fade", an ending only if they said "the end".
fn drop_unasked_fades(script: &str, ramble: &str) -> String {
    let said = ramble.to_lowercase();
    let (fades, ending) = (said.contains("fade"), said.contains("the end"));
    script
        .lines()
        .filter(|l| {
            let l = l.trim().trim_end_matches(['.', ':']).to_uppercase();
            let fade = matches!(l.as_str(), "FADE OUT" | "FADE IN" | "FADE TO BLACK");
            let end = matches!(l.as_str(), "THE END" | "END");
            !(fade && !fades) && !(end && !ending)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// What a screenplay line is, for spacing it.
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Cue,
    Parenthetical,
    Dialogue,
    Other,
}

/// Words that make a line in capitals a camera direction, not a name.
const CAMERA_WORDS: [&str; 12] = [
    "CAMERA", "PAN", "ZOOM", "CLOSE", "ANGLE", "POV", "INSERT", "TRACKING", "WIDE", "SHOT", "MONTAGE", "SUPER",
];

/// A character cue: a short line in capitals ("MAYA", "AKBAR/MUGGER",
/// "MR. OKAFOR (V.O.)") that is not a scene heading, a transition or a
/// camera direction ("PAN TO THE PHONE").
fn is_cue(line: &str) -> bool {
    let letters: Vec<char> = line.chars().filter(|c| c.is_alphabetic()).collect();
    // The name without an extension like (V.O.) or (CONT'D).
    let name = line.split('(').next().unwrap_or(line);
    let words: Vec<&str> = name.split(|c: char| c.is_whitespace() || c == '/').filter(|w| !w.is_empty()).collect();
    !letters.is_empty()
        && letters.iter().all(|c| c.is_uppercase())
        && !words.is_empty()
        && words.len() <= 4
        && !words.iter().any(|w| CAMERA_WORDS.contains(&w.trim_matches(|c: char| !c.is_alphabetic())))
        && !line.starts_with("INT")
        && !line.starts_with("EXT")
        && !line.starts_with("I/E")
        && !line.ends_with(':')
        && !line.ends_with('.')
}

/// Puts the screenplay in Fountain's shape whatever spacing the model used:
/// drops wrappers (code fences, a "Here is" line), trims lines, and puts one
/// blank line between elements but none inside a speech (a cue, its
/// parenthetical and its dialogue). Ends with a newline as Fountain files do.
pub fn tidy(text: &str) -> String {
    let mut lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.starts_with("```") && !l.is_empty())
        .collect();
    if lines.first().is_some_and(|l| {
        let l = l.to_lowercase();
        l.starts_with("here") && l.ends_with(':')
    }) {
        lines.remove(0);
    }
    let mut out = String::new();
    let mut previous: Option<Kind> = None;
    for line in lines {
        let kind = match previous {
            Some(Kind::Cue) if line.starts_with('(') => Kind::Parenthetical,
            Some(Kind::Cue | Kind::Parenthetical) => Kind::Dialogue,
            _ if is_cue(line) => Kind::Cue,
            _ => Kind::Other,
        };
        let inside_speech = matches!(kind, Kind::Parenthetical | Kind::Dialogue);
        if previous.is_some() && !inside_speech {
            out.push('\n');
        }
        out.push_str(line);
        out.push('\n');
        previous = Some(kind);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_carries_the_ramble_and_thinking_switch() {
        let p = prompt("  so um she walks in  ", false);
        assert!(p.contains("Here is what I said:\n\nso um she walks in<|im_end|>"));
        assert!(p.ends_with("<|im_start|>assistant\n"));
        assert!(prompt("x", true).ends_with("<think>\n\n</think>\n\n"));
    }

    #[test]
    fn tidy_strips_wrappers_and_extra_blank_lines() {
        let raw = "Here is the screenplay:\n```fountain\nINT. KITCHEN - NIGHT\n\n\n\nMaya stares at the phone.   \n\n  MAYA\nPick up.\n```\n";
        assert_eq!(tidy(raw), "INT. KITCHEN - NIGHT\n\nMaya stares at the phone.\n\nMAYA\nPick up.\n");
        assert_eq!(tidy("\n\n"), "");
    }

    #[test]
    fn cues_are_names_not_camera_directions() {
        for cue in ["MAYA", "MAN 1", "AKBAR/MUGGER", "MR. OKAFOR (V.O.)", "MARION (CONT'D)"] {
            assert!(is_cue(cue), "{cue}");
        }
        for not in [
            "PAN TO THE RINGING PHONE",
            "CAMERA FOLLOWS THE HAND",
            "CLOSE ON HIS FACE",
            "INT. CAR - 1:04 PM",
            "CUT TO:",
            "She runs.",
            "THE GIRL SCREAMS AT HIM AND RUNS AWAY",
        ] {
            assert!(!is_cue(not), "{not}");
        }
    }

    #[test]
    fn camera_directions_and_shouting_keep_their_place() {
        let raw = "INT. GOVERNMENT OFFICE - 1:11 PM\nA phone RINGS.\nPAN TO THE RINGING PHONE\nAKBAR\n(panicked)\nBOSS! MEIN PHASS GAYA HOON\nMAN\nHello?";
        assert_eq!(
            tidy(raw),
            "INT. GOVERNMENT OFFICE - 1:11 PM\n\nA phone RINGS.\n\nPAN TO THE RINGING PHONE\n\nAKBAR\n(panicked)\nBOSS! MEIN PHASS GAYA HOON\n\nMAN\nHello?\n"
        );
    }

    #[test]
    fn an_ending_stays_only_when_asked_for() {
        let script = "She leaves.\nTHE END";
        assert_eq!(drop_unasked_fades(script, "she leaves"), "She leaves.");
        assert_eq!(drop_unasked_fades(script, "she leaves and that's the end"), script);
    }

    #[test]
    fn fades_stay_only_when_asked_for() {
        let script = "INT. ROOM - DAY\nShe leaves.\nFADE OUT.";
        assert_eq!(drop_unasked_fades(script, "she leaves"), "INT. ROOM - DAY\nShe leaves.");
        assert_eq!(drop_unasked_fades(script, "she leaves, then fade out"), script);
    }

    #[test]
    fn tidy_spaces_elements_but_keeps_speeches_together() {
        let raw = "INT. CLASSROOM - DAY\nLeo turns the paper over.\nMR. OKAFOR\n(quietly)\nSee me after class.\nJADA\nNo way.\nCUT TO:\nEXT. CAR PARK - NIGHT";
        assert_eq!(
            tidy(raw),
            "INT. CLASSROOM - DAY\n\nLeo turns the paper over.\n\nMR. OKAFOR\n(quietly)\nSee me after class.\n\nJADA\nNo way.\n\nCUT TO:\n\nEXT. CAR PARK - NIGHT\n"
        );
    }
}

/// Writes screenplays from sample rambles with the installed word fixer and
/// prints them with their timing:
///
///   MURMUR_FIXUP_ID=<model> cargo test --release screenplay_samples -- --ignored --nocapture
#[cfg(test)]
mod eval {
    use super::*;
    use crate::engine::{default_threads, onnx};

    const RAMBLES: [&str; 4] = [
        "Okay so the scene opens in a kitchen, it's late at night, like two in the morning. Maya is sitting at the table, she's staring at her phone, waiting for her brother to call back. Um, the kettle starts whistling and she doesn't even move. Then the phone buzzes and it's Daniel and she picks up and says, where are you, I've been calling for hours. And Daniel says, I can't tell you yet, just don't open the door for anyone. Actually no, he should say don't open the door for anyone tonight. And then she looks at the front door and the scene ends.",
        "Exterior, a parking lot outside a hospital, morning, it's raining. Two characters, Sam and Priya. Sam is a doctor, he's just finished a night shift, he's exhausted. Priya runs up to him with an umbrella. She says you look terrible. He laughs and says thanks, that's exactly what I needed to hear. Then she tells him the board approved the new wing. He just stands there in the rain, he can't believe it. Then cut to inside his car, he's sitting there, and he finally starts crying. I want that moment to be quiet, no music.",
        "So I'm thinking for this one, we're in a classroom, day time, a teacher named Mr. Okafor is handing back exams. The whole class is quiet. He gets to a kid at the back, Leo, and he kind of pauses and he puts the paper face down. Leo turns it over and it's a perfect score. And Mr. Okafor just says, quietly, see me after class. Um, I don't know, maybe that's too mysterious, but yeah let's keep it. Leo looks at his friend Jada and she shrugs.",
        "Okay so we're in a car, it's like one in the afternoon, a girl is sitting in the driver's seat crying, and a mugger opens the back door and gets in. He points the gun at her and says saamaan nikaalo. And she just starts throwing everything at him and she's screaming yeh lo phone, chaabi, sab le lo. Camera stays on his face, he looks really confused. Then he says, very softly, behen main sirf lootne aaya tha.",
    ];

    #[test]
    #[ignore = "needs a downloaded word fixer"]
    fn screenplay_samples() {
        onnx::use_vendored_runtime();
        let id = std::env::var("MURMUR_FIXUP_ID").unwrap_or_else(|_| "qwen3-4b-instruct-int4".into());
        let thinking = crate::models::catalog::catalog().into_iter().any(|m| m.id == id && m.thinking);
        let llm = Llm::load(&onnx::installed_model_dir(&id), default_threads()).unwrap();
        for ramble in RAMBLES {
            let started = std::time::Instant::now();
            let mut tokens = 0;
            let script = write(&llm, thinking, ramble, |_| {
                tokens += 1;
                true
            })
            .unwrap();
            let secs = started.elapsed().as_secs_f64();
            println!("\n=== {tokens} tokens in {secs:.1} s ===\n{script}");
        }
    }
}
