import type { Metadata, Viewport } from 'next';
import './globals.css';
import '@near-wallet-selector/modal-ui/styles.css';

export const metadata: Metadata = {
  title: 'Drizzle ☔ $DRIZZLE — the memecoin printed by the sky',
  description:
    'Plant $DRIZZLE in one of 12 real cities. It only mints when it actually rains there, read by an attested OutLayer TEE oracle. Dry rounds evaporate.',
};

export const viewport: Viewport = {
  width: 'device-width',
  initialScale: 1,
  themeColor: '#050b16',
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en">
      <body>{children}</body>
    </html>
  );
}
