import { useEffect, useState } from "react";
import { Button, Card, Form } from "react-bootstrap";
import {
  ACTIONS,
  bindingFromChord,
  chordsFor,
  eventToChord,
  FIXED_BINDINGS,
  hintsEnabled,
  setHintsEnabled,
  formatChord,
  isOverridden,
  resetAll,
  resetBinding,
  setBinding,
  useKeybindings,
  validate,
  type Action,
  type Section,
} from "../../lib/keybindings";

const SECTIONS: Section[] = ["Navigation", "Hosts", "Terminal tabs"];

function Chords({ chords }: { chords: string[] }) {
  return (
    <span className="d-inline-flex gap-2 flex-wrap justify-content-end">
      {chords.map((chord) => (
        <kbd key={chord}>{formatChord(chord)}</kbd>
      ))}
    </span>
  );
}

export function KeybindingsPanel() {
  useKeybindings();
  const [recording, setRecording] = useState<string | null>(null);
  const [error, setError] = useState<{ id: string; message: string } | null>(null);

  // Window capture runs before the app's document listeners, so a chord being
  // recorded never triggers its current action.
  useEffect(() => {
    if (!recording) return;
    const onKeyDown = (event: KeyboardEvent) => {
      event.preventDefault();
      event.stopPropagation();
      if (event.key === "Escape") {
        setRecording(null);
        setError(null);
        return;
      }
      const chord = eventToChord(event);
      if (!chord) return;
      const problem = validate(recording, chord);
      if (problem) {
        setError({ id: recording, message: problem });
        return;
      }
      setBinding(recording, [bindingFromChord(recording, chord)]);
      setRecording(null);
      setError(null);
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [recording]);

  const row = (action: Action) => {
    const active = recording === action.id;
    const chords = chordsFor(action.id);
    return (
      <div key={action.id} className="py-1">
        <div className="d-flex justify-content-between align-items-center gap-3">
          <span>{action.label}</span>
          <span className="d-flex align-items-center gap-2">
            {active ? (
              <span className="text-body-secondary small">Press the new keys, Esc to cancel</span>
            ) : (
              <Chords
                chords={action.id === "hosts" ? chords.map((mod) => `${mod}+1..9`) : chords}
              />
            )}
            <Button
              size="sm"
              variant="outline-secondary"
              onClick={() => {
                setError(null);
                setRecording(active ? null : action.id);
              }}
            >
              {active ? "Cancel" : "Change"}
            </Button>
            {isOverridden(action.id) && (
              <Button
                size="sm"
                variant="link"
                onClick={() => {
                  resetBinding(action.id);
                  setRecording(null);
                }}
              >
                Reset
              </Button>
            )}
          </span>
        </div>
        {error?.id === action.id && (
          <div className="text-danger small text-end">{error.message}</div>
        )}
      </div>
    );
  };

  return (
    <>
      <Form.Check
        type="switch"
        id="key-hints"
        className="mb-2"
        label="Show shortcut hints in the sidebar and on buttons"
        checked={hintsEnabled()}
        onChange={(event) => setHintsEnabled(event.target.checked)}
      />
      <div className="d-flex justify-content-between align-items-center mb-3">
        <p className="text-body-secondary mb-0">Click Change, then press the new keys.</p>
        <Button
          size="sm"
          variant="outline-secondary"
          onClick={() => {
            resetAll();
            setRecording(null);
          }}
        >
          Reset all
        </Button>
      </div>

      {SECTIONS.map((section) => (
        <Card key={section} className="mb-3">
          <Card.Body>
            <h2 className="section-title mb-2">{section}</h2>
            {ACTIONS.filter((action) => action.section === section).map(row)}
            {FIXED_BINDINGS.filter((fixed) => fixed.section === section).map((fixed) => (
              <div key={fixed.label} className="d-flex justify-content-between align-items-center py-1">
                <span>{fixed.label}</span>
                <Chords chords={fixed.chords} />
              </div>
            ))}
          </Card.Body>
        </Card>
      ))}

      <Card className="mb-3">
        <Card.Body>
          <h2 className="section-title mb-2">Terminal clipboard</h2>
          {FIXED_BINDINGS.filter((fixed) => fixed.section === "Terminal clipboard").map((fixed) => (
            <div key={fixed.label} className="d-flex justify-content-between align-items-center py-1">
              <span>{fixed.label}</span>
              <Chords chords={fixed.chords} />
            </div>
          ))}
        </Card.Body>
      </Card>
    </>
  );
}
