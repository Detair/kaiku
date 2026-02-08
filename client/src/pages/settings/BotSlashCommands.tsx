/**
 * Bot Slash Commands Management Page
 */

import { Component, createSignal, For, Show, onMount } from 'solid-js';
import { A, useParams } from '@solidjs/router';
import { ArrowLeft, Plus, Trash2, Terminal } from 'lucide-solid';
import {
  getBotApplication,
  listSlashCommands,
  registerSlashCommands,
  deleteSlashCommand,
  deleteAllSlashCommands,
  type BotApplication,
  type SlashCommand,
  type CommandOption,
} from '../../lib/api/bots';
import { showToast } from '../../components/ui/Toast';

const OPTION_TYPES: CommandOption['type'][] = ['string', 'integer', 'boolean', 'user', 'channel', 'role'];

const BotSlashCommands: Component = () => {
  const params = useParams<{ id: string }>();
  const [app, setApp] = createSignal<BotApplication | null>(null);
  const [commands, setCommands] = createSignal<SlashCommand[]>([]);
  const [loading, setLoading] = createSignal(true);
  const [showCreateModal, setShowCreateModal] = createSignal(false);

  // Create form state
  const [newName, setNewName] = createSignal('');
  const [newDescription, setNewDescription] = createSignal('');
  const [newOptions, setNewOptions] = createSignal<CommandOption[]>([]);

  onMount(() => {
    loadData();
  });

  async function loadData() {
    try {
      setLoading(true);
      const [application, cmds] = await Promise.all([
        getBotApplication(params.id),
        listSlashCommands(params.id),
      ]);
      setApp(application);
      setCommands(cmds);
    } catch (error) {
      showToast({
        type: 'error',
        title: 'Failed to load data',
        message: error instanceof Error ? error.message : 'Failed to load data',
      });
    } finally {
      setLoading(false);
    }
  }

  async function loadCommands() {
    try {
      const cmds = await listSlashCommands(params.id);
      setCommands(cmds);
    } catch (error) {
      showToast({
        type: 'error',
        title: 'Failed to load commands',
        message: error instanceof Error ? error.message : 'Failed to load commands',
      });
    }
  }

  function addOption() {
    setNewOptions([...newOptions(), { name: '', description: '', type: 'string', required: false }]);
  }

  function removeOption(index: number) {
    setNewOptions(newOptions().filter((_, i) => i !== index));
  }

  function updateOption(index: number, field: keyof CommandOption, value: string | boolean) {
    setNewOptions(
      newOptions().map((opt, i) =>
        i === index ? { ...opt, [field]: value } : opt
      )
    );
  }

  function resetForm() {
    setNewName('');
    setNewDescription('');
    setNewOptions([]);
  }

  async function handleCreateCommand() {
    const name = newName().trim();
    const description = newDescription().trim();

    if (!name) {
      showToast({ type: 'error', title: 'Invalid command', message: 'Command name is required' });
      return;
    }
    if (!description) {
      showToast({ type: 'error', title: 'Invalid command', message: 'Command description is required' });
      return;
    }

    // Validate options — reject if any option has partial data (name but no description or vice versa)
    const opts = newOptions();
    for (const opt of opts) {
      const hasName = opt.name.trim().length > 0;
      const hasDesc = opt.description.trim().length > 0;
      if (hasName !== hasDesc) {
        showToast({ type: 'error', title: 'Invalid option', message: 'All options must have both a name and description' });
        return;
      }
    }
    const options = opts.filter(o => o.name.trim() && o.description.trim());

    try {
      await registerSlashCommands(params.id, {
        commands: [{
          name,
          description,
          options: options.length > 0 ? options : undefined,
        }],
      });
      setShowCreateModal(false);
      resetForm();
      showToast({ type: 'success', title: 'Command registered', message: 'Command registered successfully' });
      loadCommands();
    } catch (error) {
      showToast({
        type: 'error',
        title: 'Failed to register command',
        message: error instanceof Error ? error.message : 'Failed to register command',
      });
    }
  }

  async function handleDeleteCommand(commandId: string, commandName: string) {
    if (!confirm(`Delete command "/${commandName}"? This cannot be undone!`)) return;

    try {
      await deleteSlashCommand(params.id, commandId);
      showToast({ type: 'success', title: 'Command deleted', message: 'Command deleted successfully' });
      loadCommands();
    } catch (error) {
      showToast({
        type: 'error',
        title: 'Failed to delete command',
        message: error instanceof Error ? error.message : 'Failed to delete command',
      });
    }
  }

  async function handleDeleteAll() {
    if (!confirm('Delete ALL commands? This cannot be undone!')) return;

    try {
      await deleteAllSlashCommands(params.id);
      showToast({ type: 'success', title: 'All commands deleted', message: 'All commands deleted successfully' });
      loadCommands();
    } catch (error) {
      showToast({
        type: 'error',
        title: 'Failed to delete commands',
        message: error instanceof Error ? error.message : 'Failed to delete commands',
      });
    }
  }

  return (
    <div class="min-h-screen bg-surface-base text-text-primary p-6">
      <div class="max-w-4xl mx-auto">
        {/* Header */}
        <div class="flex items-center gap-4 mb-6">
          <A href="/" class="p-2 hover:bg-surface-layer1 rounded-lg">
            <ArrowLeft class="w-5 h-5" />
          </A>
          <div class="flex-1">
            <h1 class="text-xl font-semibold">
              {app()?.name ?? 'Bot'} — Slash Commands
            </h1>
            <Show when={!loading()}>
              <p class="text-sm text-text-secondary">
                {commands().length} command{commands().length !== 1 ? 's' : ''} registered
              </p>
            </Show>
          </div>
        </div>

        <Show when={loading()}>
          <div class="text-center py-8 text-text-secondary">Loading commands...</div>
        </Show>

        <Show when={!loading()}>
          {/* Actions */}
          <div class="mb-4 flex gap-2">
            <button
              class="flex items-center gap-2 px-4 py-2 bg-primary-600 hover:bg-primary-700 rounded-md transition-colors"
              onClick={() => setShowCreateModal(true)}
            >
              <Plus class="w-4 h-4" />
              Register Command
            </button>
            <Show when={commands().length > 0}>
              <button
                class="flex items-center gap-2 px-4 py-2 bg-surface-700 hover:bg-surface-600 rounded-md transition-colors text-red-400"
                onClick={handleDeleteAll}
              >
                <Trash2 class="w-4 h-4" />
                Delete All
              </button>
            </Show>
          </div>

          {/* Empty state */}
          <Show when={commands().length === 0}>
            <div class="text-center py-16">
              <Terminal class="w-12 h-12 mx-auto mb-4 text-text-secondary" />
              <div class="text-lg font-medium mb-2">No commands registered</div>
              <div class="text-text-secondary">
                Register a slash command to get started.
              </div>
            </div>
          </Show>

          {/* Command list */}
          <div class="space-y-4">
            <For each={commands()}>
              {(cmd) => (
                <div class="bg-surface-800 rounded-lg p-4 border border-surface-700">
                  <div class="flex justify-between items-start">
                    <div>
                      <h3 class="text-lg font-semibold">/{cmd.name}</h3>
                      <p class="text-sm text-surface-400 mt-1">{cmd.description}</p>
                      <p class="text-xs text-surface-500 mt-1">
                        Created: {new Date(cmd.created_at).toLocaleDateString()}
                      </p>
                    </div>
                    <button
                      class="p-2 text-surface-400 hover:text-red-400 hover:bg-surface-700 rounded transition-colors"
                      onClick={() => handleDeleteCommand(cmd.id, cmd.name)}
                      title="Delete command"
                    >
                      <Trash2 class="w-4 h-4" />
                    </button>
                  </div>

                  {/* Options */}
                  <Show when={cmd.options && cmd.options.length > 0}>
                    <div class="mt-3 pt-3 border-t border-surface-700">
                      <p class="text-xs font-medium text-surface-400 mb-2">Options</p>
                      <div class="flex flex-wrap gap-2">
                        <For each={cmd.options}>
                          {(opt) => (
                            <span class="inline-flex items-center gap-1 px-2 py-1 bg-surface-700 rounded text-xs">
                              <span class="font-medium">{opt.name}</span>
                              <span class="text-surface-400">({opt.type})</span>
                              <Show when={opt.required}>
                                <span class="text-red-400">*</span>
                              </Show>
                            </span>
                          )}
                        </For>
                      </div>
                    </div>
                  </Show>
                </div>
              )}
            </For>
          </div>
        </Show>

        {/* Create Command Modal */}
        <Show when={showCreateModal()}>
          <div
            class="fixed inset-0 bg-black/50 flex items-center justify-center z-50"
            onClick={() => { setShowCreateModal(false); resetForm(); }}
          >
            <div
              class="bg-surface-800 rounded-lg p-6 w-full max-w-lg max-h-[80vh] overflow-y-auto"
              onClick={(e) => e.stopPropagation()}
            >
              <h2 class="text-xl font-bold mb-4">Register Slash Command</h2>
              <div class="space-y-4">
                <div>
                  <label class="block text-sm font-medium mb-1">Name *</label>
                  <input
                    type="text"
                    class="w-full px-3 py-2 bg-surface-900 border border-surface-700 rounded focus:outline-none focus:ring-2 focus:ring-primary-500"
                    placeholder="ping"
                    value={newName()}
                    onInput={(e) => setNewName(e.currentTarget.value)}
                  />
                  <p class="text-xs text-surface-500 mt-1">Lowercase, no spaces (e.g. "ping", "roll-dice")</p>
                </div>
                <div>
                  <label class="block text-sm font-medium mb-1">Description *</label>
                  <input
                    type="text"
                    class="w-full px-3 py-2 bg-surface-900 border border-surface-700 rounded focus:outline-none focus:ring-2 focus:ring-primary-500"
                    placeholder="Replies with pong!"
                    value={newDescription()}
                    onInput={(e) => setNewDescription(e.currentTarget.value)}
                    maxLength={100}
                  />
                  <p class="text-xs text-surface-500 mt-1">Max 100 characters</p>
                </div>

                {/* Options */}
                <div>
                  <div class="flex items-center justify-between mb-2">
                    <label class="text-sm font-medium">Options</label>
                    <button
                      class="flex items-center gap-1 px-2 py-1 text-xs bg-surface-700 hover:bg-surface-600 rounded transition-colors"
                      onClick={addOption}
                    >
                      <Plus class="w-3 h-3" />
                      Add Option
                    </button>
                  </div>
                  <div class="space-y-3">
                    <For each={newOptions()}>
                      {(opt, index) => (
                        <div class="bg-surface-900 rounded p-3 border border-surface-700">
                          <div class="flex justify-between items-start mb-2">
                            <span class="text-xs text-surface-400">Option {index() + 1}</span>
                            <button
                              class="p-1 text-surface-400 hover:text-red-400 rounded transition-colors"
                              onClick={() => removeOption(index())}
                            >
                              <Trash2 class="w-3 h-3" />
                            </button>
                          </div>
                          <div class="grid grid-cols-2 gap-2 mb-2">
                            <input
                              type="text"
                              class="px-2 py-1 text-sm bg-surface-800 border border-surface-700 rounded focus:outline-none focus:ring-1 focus:ring-primary-500"
                              placeholder="Name"
                              value={opt.name}
                              onInput={(e) => updateOption(index(), 'name', e.currentTarget.value)}
                            />
                            <select
                              class="px-2 py-1 text-sm bg-surface-800 border border-surface-700 rounded focus:outline-none focus:ring-1 focus:ring-primary-500"
                              value={opt.type}
                              onChange={(e) => updateOption(index(), 'type', e.currentTarget.value)}
                            >
                              <For each={OPTION_TYPES}>
                                {(t) => <option value={t}>{t}</option>}
                              </For>
                            </select>
                          </div>
                          <input
                            type="text"
                            class="w-full px-2 py-1 text-sm bg-surface-800 border border-surface-700 rounded focus:outline-none focus:ring-1 focus:ring-primary-500 mb-2"
                            placeholder="Description"
                            value={opt.description}
                            onInput={(e) => updateOption(index(), 'description', e.currentTarget.value)}
                          />
                          <label class="flex items-center gap-2 text-sm">
                            <input
                              type="checkbox"
                              checked={opt.required}
                              onChange={(e) => updateOption(index(), 'required', e.currentTarget.checked)}
                              class="rounded"
                            />
                            Required
                          </label>
                        </div>
                      )}
                    </For>
                  </div>
                </div>

                <div class="flex gap-2 justify-end">
                  <button
                    class="px-4 py-2 bg-surface-700 hover:bg-surface-600 rounded transition-colors"
                    onClick={() => { setShowCreateModal(false); resetForm(); }}
                  >
                    Cancel
                  </button>
                  <button
                    class="px-4 py-2 bg-primary-600 hover:bg-primary-700 rounded transition-colors"
                    onClick={handleCreateCommand}
                  >
                    Register
                  </button>
                </div>
              </div>
            </div>
          </div>
        </Show>
      </div>
    </div>
  );
};

export default BotSlashCommands;
