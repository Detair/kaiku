import { Component, Show } from "solid-js";
import { Hash } from "lucide-solid";
import Sidebar from "@/components/layout/Sidebar";
import MessageList from "@/components/messages/MessageList";
import MessageInput from "@/components/messages/MessageInput";
import TypingIndicator from "@/components/messages/TypingIndicator";
import { selectedChannel } from "@/stores/channels";

const Main: Component = () => {
  const channel = selectedChannel;

  return (
    <div class="flex h-screen bg-background-primary">
      {/* Sidebar */}
      <Sidebar />

      {/* Main Content */}
      <main class="flex-1 flex flex-col min-w-0">
        <Show
          when={channel()}
          fallback={
            <div class="flex-1 flex items-center justify-center">
              <div class="text-center text-text-muted">
                <Hash class="w-12 h-12 mx-auto mb-4 opacity-50" />
                <p class="text-lg">Select a channel to start chatting</p>
              </div>
            </div>
          }
        >
          {/* Channel Header */}
          <header class="h-12 px-4 flex items-center border-b border-background-tertiary bg-background-primary shadow-sm">
            <Hash class="w-5 h-5 text-text-muted mr-2" />
            <span class="font-semibold text-text-primary">{channel()?.name}</span>
            <Show when={channel()?.topic}>
              <div class="ml-4 pl-4 border-l border-background-tertiary text-text-secondary text-sm truncate">
                {channel()?.topic}
              </div>
            </Show>
          </header>

          {/* Messages */}
          <MessageList channelId={channel()!.id} />

          {/* Typing Indicator */}
          <TypingIndicator channelId={channel()!.id} />

          {/* Message Input */}
          <MessageInput channelId={channel()!.id} channelName={channel()!.name} />
        </Show>
      </main>
    </div>
  );
};

export default Main;
