import { Container, Nav, Navbar } from "react-bootstrap";
import { ThemeToggle } from "../theme/ThemeToggle";

export function AppNavbar() {
  return (
    <Navbar expand="md" className="bg-body-tertiary border-bottom" sticky="top">
      <Container fluid>
        <Navbar.Brand className="d-flex align-items-center gap-2 fw-semibold">
          <i className="bi bi-terminal-fill text-primary" aria-hidden="true" />
          ParolaSSH
        </Navbar.Brand>

        <Navbar.Toggle aria-controls="main-nav" />
        <Navbar.Collapse id="main-nav">
          <Nav className="me-auto">
            <Nav.Link active>Hosts</Nav.Link>
            <Nav.Link disabled>Sessions</Nav.Link>
            <Nav.Link disabled>Keys</Nav.Link>
            <Nav.Link disabled>Settings</Nav.Link>
          </Nav>

          <div className="d-flex align-items-center gap-2">
            <ThemeToggle />
          </div>
        </Navbar.Collapse>
      </Container>
    </Navbar>
  );
}
