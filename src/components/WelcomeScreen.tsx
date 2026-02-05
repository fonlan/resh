import React from 'react';
import { Server as ServerIcon, Plus, Terminal } from 'lucide-react';
import { Server } from '../types';
import { useTranslation } from '../i18n';
import { EmojiText } from './EmojiText';

interface WelcomeScreenProps {
  servers: Server[];
  onServerClick: (serverId: string) => void;
  onOpenSettings: () => void;
  onServerContextMenu: (e: React.MouseEvent, serverId: string) => void;
}

export const WelcomeScreen: React.FC<WelcomeScreenProps> = ({
  servers,
  onServerClick,
  onOpenSettings,
  onServerContextMenu,
}) => {
  const { t } = useTranslation();
  const hasServers = servers.length > 0;

  return (
    <div className="w-full h-full flex flex-col items-center justify-center bg-[var(--bg-primary)] overflow-y-auto">
      <div className="max-w-[800px] w-full px-5 py-10 flex flex-col">
        <div className="text-center mb-16">
          <div className="inline-flex items-center justify-center w-20 h-20 bg-[var(--bg-secondary)] border border-[var(--glass-border)] rounded-[var(--radius-lg)] text-[var(--accent-primary)] mb-6 shadow-[var(--glow-primary)] transition-transform duration-300 hover:scale-105 hover:rotate-5">
            <Terminal size={48} />
          </div>
          <h1 className="text-[36px] font-extrabold text-[var(--text-primary)] my-0 mx-0 mb-2  leading-[1.2] font-[var(--font-display)]">
            {t.welcome.title}
          </h1>
          <p className="text-base text-[var(--text-secondary)] my-0 leading-[1.6]">
            {t.welcome.subtitle}
          </p>
        </div>

        {hasServers ? (
          <div className="mb-10">
            <div className="flex items-center justify-between mb-5">
              <h2 className="text-[14px] font-semibold text-zinc-500 uppercase  my-0 leading-[1.4]">
                {t.welcome.recentTitle}
              </h2>
              <button type="button" className="bg-transparent border-0 text-[var(--accent-primary)] text-[13px] font-semibold cursor-pointer px-2 py-1 rounded transition-all hover:bg-[rgba(59,130,246,0.1)]" onClick={onOpenSettings}>
                {t.welcome.viewAll}
              </button>
            </div>
            <div className="grid grid-cols-[repeat(auto-fill,minmax(240px,1fr))] gap-3">
              {servers.map((server) => (
                <button
                  type="button"
                  key={server.id}
                  className="flex flex-row items-center justify-start gap-4 px-5 py-4 bg-[var(--bg-secondary)] border border-[var(--glass-border)] rounded-[var(--radius-md)] cursor-pointer transition-all text-left hover:bg-[var(--bg-tertiary)] hover:border-[var(--accent-primary)] hover:-translate-y-0.5 hover:shadow-[0_8px_24px_rgba(0,0,0,0.3)]"
                  onClick={() => onServerClick(server.id)}
                  onContextMenu={(e) => onServerContextMenu(e, server.id)}
                >
                  <div className="w-10 h-10 flex items-center justify-center bg-[var(--bg-primary)] border border-[var(--glass-border)] rounded-[var(--radius-sm)] text-[var(--text-secondary)] flex-shrink-0 group-hover:text-[var(--accent-primary)] group-hover:border-[rgba(59,130,246,0.3)] group-hover:shadow-[var(--glow-primary)]">
                    <ServerIcon size={20} />
                  </div>
                  <div className="flex flex-col items-start min-w-0 flex-1">
                    <h3 className="text-[14px] font-semibold text-[var(--text-primary)] my-0 mx-0 mb-0.5 whitespace-nowrap overflow-hidden text-ellipsis leading-[1.4]">
                      <EmojiText text={server.name} />
                    </h3>
                    <p className="text-xs text-zinc-500 my-0 whitespace-nowrap overflow-hidden text-ellipsis leading-[1.5] w-full">
                      {server.username ? `${server.username}@` : ''}{server.host}
                    </p>
                  </div>
                </button>
              ))}
            </div>
          </div>
        ) : (
          <div className="text-center px-12 py-12 bg-[var(--bg-secondary)] rounded-[var(--radius-lg)] border border-[var(--glass-border)]">
            <div className="w-16 h-16 mx-auto mb-6 flex items-center justify-center bg-[var(--bg-primary)] border border-[var(--glass-border)] rounded-full text-zinc-500">
              <ServerIcon size={32} />
            </div>
            <h3 className="text-[18px] font-semibold text-[var(--text-primary)] my-0 mx-0 mb-2 leading-[1.4]">
              {t.welcome.noServers}
            </h3>
            <p className="text-[14px] text-[var(--text-secondary)] my-0 mx-0 mb-8 leading-[1.6]">
              {t.welcome.getFirstStarted}
            </p>
            <button type="button" className="inline-flex items-center gap-2 px-6 py-2.5 bg-[var(--accent-success)] text-white border-0 rounded-[var(--radius-md)] text-[14px] font-semibold cursor-pointer transition-all shadow-[var(--glow-success)]  leading-[1.4] hover:brightness-110 hover:-translate-y-px" onClick={onOpenSettings}>
              <Plus size={18} />
              <span>{t.serverTab.addServer}</span>
            </button>
          </div>
        )}
      </div>
    </div>
  );
};
