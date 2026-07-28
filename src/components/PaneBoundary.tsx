import { Component, type ErrorInfo, type ReactNode } from "react";
import { Alert, Button } from "react-bootstrap";

interface Props {
  /** Remount key - switching panes clears a previous crash. */
  resetKey: string;
  children: ReactNode;
}

interface State {
  error: Error | null;
}

/** Keeps a throwing pane from unmounting the whole app. React has no hook
 *  equivalent, so this stays a class. */
export class PaneBoundary extends Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidUpdate(prev: Props) {
    if (prev.resetKey !== this.props.resetKey && this.state.error) {
      this.setState({ error: null });
    }
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("Pane crashed:", error, info.componentStack);
  }

  render() {
    const { error } = this.state;
    if (!error) return this.props.children;

    return (
      <Alert variant="danger" className="mb-0">
        <Alert.Heading className="h6">This pane stopped rendering.</Alert.Heading>
        <p className="text-prewrap small mb-2">{error.message}</p>
        <Button size="sm" variant="outline-danger" onClick={() => this.setState({ error: null })}>
          Try again
        </Button>
      </Alert>
    );
  }
}
