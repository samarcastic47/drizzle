'use client';

/**
 * NEAR Wallet Selector wiring. Everything is imported lazily: the selector and the
 * modal touch `window` at import time, and the page is statically exported.
 */

import type { WalletSelector } from '@near-wallet-selector/core';
import type { WalletSelectorModal } from '@near-wallet-selector/modal-ui';
import { GAS_100_TGAS, type Signer, type TxRequest } from './contract';

let selectorPromise: Promise<WalletSelector> | null = null;
let modal: WalletSelectorModal | null = null;
let modalContractId: string | null = null;
let walletAccountId: string | null = null;

export async function getSelector(): Promise<WalletSelector> {
  if (!selectorPromise) {
    selectorPromise = (async () => {
      const [core, myNearWallet, meteorWallet, hereWallet] = await Promise.all([
        import('@near-wallet-selector/core'),
        import('@near-wallet-selector/my-near-wallet'),
        import('@near-wallet-selector/meteor-wallet'),
        import('@near-wallet-selector/here-wallet'),
      ]);
      return core.setupWalletSelector({
        network: 'testnet',
        modules: [myNearWallet.setupMyNearWallet(), meteorWallet.setupMeteorWallet(), hereWallet.setupHereWallet()],
        optimizeWalletOrder: true,
      });
    })();
  }
  return selectorPromise;
}

export async function showWalletModal(contractId: string): Promise<void> {
  const selector = await getSelector();
  if (!modal || modalContractId !== contractId) {
    const { setupModal } = await import('@near-wallet-selector/modal-ui');
    modal = setupModal(selector, {
      contractId,
      theme: 'dark',
      description: 'Plant $DRIZZLE in a city. It only pays when it rains there.',
    });
    modalContractId = contractId;
  }
  modal.show();
}

export function walletAccount(): string | null {
  return walletAccountId;
}

/** Kept in sync by the provider, so the data layer can read the signer synchronously. */
export function setWalletAccount(accountId: string | null) {
  walletAccountId = accountId;
}

export async function signFunctionCall(req: TxRequest): Promise<void> {
  const selector = await getSelector();
  const state = selector.store.getState();
  const account = state.accounts.find((a) => a.active) ?? state.accounts[0];
  if (!account) throw new Error('connect a wallet first');
  const core = await import('@near-wallet-selector/core');
  const wallet = await selector.wallet();
  const outcome = await wallet.signAndSendTransaction({
    signerId: account.accountId,
    receiverId: req.contractId,
    actions: [
      core.actionCreators.functionCall(req.methodName, req.args, BigInt(GAS_100_TGAS), BigInt(req.deposit)),
    ],
  });
  const status = outcome?.status as unknown;
  if (status && typeof status === 'object' && 'Failure' in (status as Record<string, unknown>)) {
    throw new Error(`transaction failed: ${JSON.stringify((status as Record<string, unknown>).Failure)}`);
  }
}

export async function disconnectWallet(): Promise<void> {
  const selector = await getSelector();
  const wallet = await selector.wallet();
  await wallet.signOut();
  setWalletAccount(null);
}

export const walletSigner: Signer = {
  accountId: walletAccount,
  signFunctionCall,
};

/** Mock mode pretends to be a funded testnet account so the UI is fully clickable. */
export const MOCK_ACCOUNT_ID = 'drizzle-demo.testnet';
