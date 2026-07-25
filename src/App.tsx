import { Container } from "react-bootstrap";
import { AppNavbar } from "./components/AppNavbar";
import { HostsPage } from "./features/hosts/HostsPage";
import { ThemeProvider } from "./theme/ThemeProvider";

function App() {
  return (
    <ThemeProvider>
      <div className="d-flex flex-column min-vh-100 bg-body">
        <AppNavbar />
        <main className="flex-grow-1 py-4">
          <Container fluid="xxl">
            <HostsPage />
          </Container>
        </main>
      </div>
    </ThemeProvider>
  );
}

export default App;
