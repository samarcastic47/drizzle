/** OutLayer attestation links. The pattern is provisional and may be corrected later. */
export function attestationUrl(requestId: string): string {
  return `https://app.outlayer.ai/executions/${requestId}`;
}

export const OUTLAYER_APP = 'https://app.outlayer.ai';
