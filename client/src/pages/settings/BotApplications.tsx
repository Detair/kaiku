/**
 * Bot Applications Management Page
 */

import { Component, createSignal, For, Show, onMount } from 'solid-js';
import {
  createBotApplication,
  createBotUser,
  deleteBotApplication,
  listBotApplications,
  resetBotToken,
  type BotApplication,
  type BotTokenResponse,
} from '../../lib/api/bots';
import { A } from '@solidjs/router';
import { showToast } from '../../components/ui/Toast';

const BotApplications: Component = () => {
  const [applications, setApplications] = createSignal<BotApplication[]>([]);
  const [loading, setLoading] = createSignal(true);
  const [showCreateModal, setShowCreateModal] = createSignal(false);
  const [showTokenModal, setShowTokenModal] = createSignal(false);
  const [currentToken, setCurrentToken] = createSignal<BotTokenResponse | null>(null);
  const [newAppName, setNewAppName] = createSignal('');
  const [newAppDescription, setNewAppDescription] = createSignal('');

  onMount(() => {
    loadApplications();
  });

  async function loadApplications() {
    try {
      setLoading(true);
      const apps = await listBotApplications();
      setApplications(apps);
    } catch (error) {
      showToast({
        type: 'error',
        title: 'Failed to load applications',
        message: error instanceof Error ? error.message : 'Failed to load applications',
      });
    } finally {
      setLoading(false);
    }
  }

  async function handleCreateApplication() {
    const name = newAppName().trim();
    if (!name || name.length < 2 || name.length > 100) {
      showToast({ type: 'error', title: 'Invalid application name', message: 'Name must be 2-100 characters' });
      return;
    }

    try {
      await createBotApplication({
        name,
        description: newAppDescription().trim() || undefined,
      });
      setShowCreateModal(false);
      setNewAppName('');
      setNewAppDescription('');
      showToast({ type: 'success', title: 'Application created', message: 'Application created successfully' });
      loadApplications();
    } catch (error) {
      showToast({
        type: 'error',
        title: 'Failed to create application',
        message: error instanceof Error ? error.message : 'Failed to create application',
      });
    }
  }

  async function handleCreateBotUser(appId: string) {
    if (!confirm('Create bot user? The token will only be shown once!')) return;

    try {
      const tokenData = await createBotUser(appId);
      setCurrentToken(tokenData);
      setShowTokenModal(true);
      loadApplications();
    } catch (error) {
      showToast({
        type: 'error',
        title: 'Failed to create bot user',
        message: error instanceof Error ? error.message : 'Failed to create bot user',
      });
    }
  }

  async function handleResetToken(appId: string) {
    if (!confirm('Reset bot token? The old token will stop working immediately!')) return;

    try {
      const tokenData = await resetBotToken(appId);
      setCurrentToken(tokenData);
      setShowTokenModal(true);
      showToast({ type: 'success', title: 'Token reset', message: 'Token reset successfully' });
    } catch (error) {
      showToast({
        type: 'error',
        title: 'Failed to reset token',
        message: error instanceof Error ? error.message : 'Failed to reset token',
      });
    }
  }

  async function handleDeleteApplication(appId: string, appName: string) {
    if (!confirm(`Delete application "${appName}"? This cannot be undone!`)) return;

    try {
      await deleteBotApplication(appId);
      showToast({ type: 'success', title: 'Application deleted', message: 'Application deleted successfully' });
      loadApplications();
    } catch (error) {
      showToast({
        type: 'error',
        title: 'Failed to delete application',
        message: error instanceof Error ? error.message : 'Failed to delete application',
      });
    }
  }

  function copyToken() {
    const token = currentToken()?.token;
    if (token) {
      navigator.clipboard.writeText(token);
      showToast({ type: 'success', title: 'Token copied', message: 'Token copied to clipboard' });
    }
  }

  return (
    <div class="p-6">
      <div class="mb-6">
        <h1 class="text-2xl font-bold mb-2">Bot Applications</h1>
        <p class="text-surface-400">
          Create and manage bot applications for the VoiceChat platform.
        </p>
      </div>

      <div class="mb-4">
        <button
          class="px-4 py-2 bg-primary-600 hover:bg-primary-700 rounded-md transition-colors"
          onClick={() => setShowCreateModal(true)}
        >
          Create Application
        </button>
      </div>

      <Show when={loading()}>
        <div class="text-center py-8 text-surface-400">Loading applications...</div>
      </Show>

      <Show when={!loading() && applications().length === 0}>
        <div class="text-center py-8 text-surface-400">
          No applications yet. Create one to get started!
        </div>
      </Show>

      <div class="space-y-4">
        <For each={applications()}>
          {(app) => (
            <div class="bg-surface-800 rounded-lg p-4 border border-surface-700">
              <div class="flex justify-between items-start mb-2">
                <div>
                  <h3 class="text-lg font-semibold">{app.name}</h3>
                  <Show when={app.description}>
                    <p class="text-sm text-surface-400 mt-1">{app.description}</p>
                  </Show>
                  <p class="text-xs text-surface-500 mt-1">
                    Created: {new Date(app.created_at).toLocaleDateString()}
                  </p>
                </div>
                <div class="flex gap-2">
                  <Show
                    when={app.bot_user_id}
                    fallback={
                      <button
                        class="px-3 py-1 text-sm bg-green-600 hover:bg-green-700 rounded transition-colors"
                        onClick={() => handleCreateBotUser(app.id)}
                      >
                        Create Bot User
                      </button>
                    }
                  >
                    <span class="px-3 py-1 text-sm bg-green-900 text-green-200 rounded">
                      ✓ Bot Created
                    </span>
                  </Show>
                  <button
                    class="px-3 py-1 text-sm bg-surface-700 hover:bg-surface-600 rounded transition-colors"
                    onClick={() => handleDeleteApplication(app.id, app.name)}
                  >
                    Delete
                  </button>
                </div>
              </div>

              <Show when={app.bot_user_id}>
                <div class="mt-3 pt-3 border-t border-surface-700 flex gap-2">
                  <button
                    class="px-3 py-1 text-sm bg-blue-600 hover:bg-blue-700 rounded transition-colors"
                    onClick={() => handleResetToken(app.id)}
                  >
                    Reset Token
                  </button>
                  <A
                    href={`/settings/bots/${app.id}/commands`}
                    class="px-3 py-1 text-sm bg-surface-700 hover:bg-surface-600 rounded transition-colors inline-block"
                  >
                    Manage Commands
                  </A>
                </div>
              </Show>
            </div>
          )}
        </For>
      </div>

      {/* Create Application Modal */}
      <Show when={showCreateModal()}>
        <div
          class="fixed inset-0 bg-black/50 flex items-center justify-center z-50"
          onClick={() => setShowCreateModal(false)}
        >
          <div
            class="bg-surface-800 rounded-lg p-6 w-full max-w-md"
            onClick={(e) => e.stopPropagation()}
          >
            <h2 class="text-xl font-bold mb-4">Create Bot Application</h2>
            <div class="space-y-4">
              <div>
                <label class="block text-sm font-medium mb-1">Name *</label>
                <input
                  type="text"
                  class="w-full px-3 py-2 bg-surface-900 border border-surface-700 rounded focus:outline-none focus:ring-2 focus:ring-primary-500"
                  placeholder="My Cool Bot"
                  value={newAppName()}
                  onInput={(e) => setNewAppName(e.currentTarget.value)}
                  maxLength={100}
                />
                <p class="text-xs text-surface-500 mt-1">2-100 characters</p>
              </div>
              <div>
                <label class="block text-sm font-medium mb-1">Description</label>
                <textarea
                  class="w-full px-3 py-2 bg-surface-900 border border-surface-700 rounded focus:outline-none focus:ring-2 focus:ring-primary-500 resize-none"
                  placeholder="What does your bot do?"
                  rows={3}
                  value={newAppDescription()}
                  onInput={(e) => setNewAppDescription(e.currentTarget.value)}
                  maxLength={1000}
                />
                <p class="text-xs text-surface-500 mt-1">Optional, max 1000 characters</p>
              </div>
              <div class="flex gap-2 justify-end">
                <button
                  class="px-4 py-2 bg-surface-700 hover:bg-surface-600 rounded transition-colors"
                  onClick={() => setShowCreateModal(false)}
                >
                  Cancel
                </button>
                <button
                  class="px-4 py-2 bg-primary-600 hover:bg-primary-700 rounded transition-colors"
                  onClick={handleCreateApplication}
                >
                  Create
                </button>
              </div>
            </div>
          </div>
        </div>
      </Show>

      {/* Bot Token Modal */}
      <Show when={showTokenModal() && currentToken()}>
        <div
          class="fixed inset-0 bg-black/50 flex items-center justify-center z-50"
          onClick={() => setShowTokenModal(false)}
        >
          <div
            class="bg-surface-800 rounded-lg p-6 w-full max-w-lg"
            onClick={(e) => e.stopPropagation()}
          >
            <h2 class="text-xl font-bold mb-4">Bot Token</h2>
            <div class="bg-yellow-900/20 border border-yellow-600 rounded p-3 mb-4">
              <p class="text-yellow-200 text-sm">
                ⚠️ <strong>Save this token now!</strong> It will only be shown once and cannot
                be retrieved later.
              </p>
            </div>
            <div class="space-y-4">
              <div>
                <label class="block text-sm font-medium mb-1">Bot User ID</label>
                <code class="block w-full px-3 py-2 bg-surface-900 border border-surface-700 rounded text-sm">
                  {currentToken()?.bot_user_id}
                </code>
              </div>
              <div>
                <label class="block text-sm font-medium mb-1">Token</label>
                <div class="flex gap-2">
                  <code class="flex-1 px-3 py-2 bg-surface-900 border border-surface-700 rounded text-sm break-all">
                    {currentToken()?.token}
                  </code>
                  <button
                    class="px-3 py-2 bg-primary-600 hover:bg-primary-700 rounded transition-colors whitespace-nowrap"
                    onClick={copyToken}
                  >
                    Copy
                  </button>
                </div>
              </div>
              <div class="flex justify-end">
                <button
                  class="px-4 py-2 bg-surface-700 hover:bg-surface-600 rounded transition-colors"
                  onClick={() => {
                    setShowTokenModal(false);
                    setCurrentToken(null);
                  }}
                >
                  Close
                </button>
              </div>
            </div>
          </div>
        </div>
      </Show>
    </div>
  );
};

export default BotApplications;
