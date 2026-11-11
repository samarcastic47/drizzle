import { App } from '../components/App';
import { DrizzleProvider } from '../lib/useDrizzle';

export default function Page() {
  return (
    <DrizzleProvider>
      <App />
    </DrizzleProvider>
  );
}
