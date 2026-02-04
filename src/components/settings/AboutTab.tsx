import React from 'react';
import { Terminal, Github, Heart, Shield, Cpu, ExternalLink } from 'lucide-react';
import { useTranslation } from '../../i18n';
import { invoke } from '@tauri-apps/api/core';

export const AboutTab: React.FC = () => {
  const { t } = useTranslation();

  const techStack = [
    'Tauri 2.0',
    'Rust',
    'React 18',
    'TypeScript',
    'xterm.js',
    'Zustand',
    'Vite'
  ];

  const openLink = async (url: string) => {
    try {
      await invoke('open_url', { url });
    } catch (err) {
      console.error('Failed to open link via invoke:', err);
      window.open(url, '_blank');
    }
  };

  return (
    <div className="flex flex-col items-center px-4 py-8 h-full overflow-y-auto relative" style={{
      backgroundImage: 'radial-gradient(circle at 2px 2px, rgba(255, 255, 255, 0.05) 1px, transparent 0)',
      backgroundSize: '24px 24px'
    }}>
      <div className="flex flex-col items-center mb-10 text-center animate-[fadeIn_0.6s_cubic-bezier(0.22,1,0.36,1)_forwards]">
        <div className="relative mb-6 p-6 rounded-3xl shadow-[0_8px_32px_rgba(0,0,0,0.2)] border border-white/10 transition-transform duration-300 hover:scale-105 hover:rotate-5 bg-[var(--bg-secondary,#1a1b1e)]">
          <Terminal size={48} className="text-[var(--accent-color,#f97316)]" style={{ filter: 'drop-shadow(0 0 8px rgba(249, 115, 22, 0.4))' }} />
        </div>
        <h1 className="text-[2.5rem] font-extrabold m-0 tracking-tight" style={{
          background: 'linear-gradient(135deg, #fff 0%, #a1a1aa 100%)',
          WebkitBackgroundClip: 'text',
          WebkitTextFillColor: 'transparent'
        }}>
          Resh
        </h1>
        <div className="font-mono text-sm text-zinc-500 bg-white/5 px-3 py-1 rounded-full mt-2 border border-white/10">
          {t.about.version} 0.1.0
        </div>
      </div>

      <p className="max-w-[480px] text-center text-zinc-400 leading-[1.6] mb-12 text-[1.1rem] animate-[fadeIn_0.6s_cubic-bezier(0.22,1,0.36,1)_0.1s_forwards] opacity-0" style={{ animationFillMode: 'forwards' }}>
        {t.about.description}
      </p>

      <div className="grid grid-cols-2 gap-6 w-full max-w-[600px] mb-12 animate-[fadeIn_0.6s_cubic-bezier(0.22,1,0.36,1)_0.2s_forwards] opacity-0" style={{ animationFillMode: 'forwards' }}>
        <div className="bg-white/[0.03] border border-white/[0.05] p-5 rounded-2xl transition-all duration-200 hover:bg-white/[0.05] hover:border-white/10 hover:-translate-y-0.5 flex flex-col gap-2">
          <span className="text-[0.75rem] uppercase tracking-[0.05em] text-zinc-500 font-semibold">
            {t.about.author}
          </span>
          <div className="text-base text-zinc-100 font-medium flex items-center min-h-6">
            <span className="font-bold">fonlan</span>
          </div>
        </div>

        <div className="bg-white/[0.03] border border-white/[0.05] p-5 rounded-2xl transition-all duration-200 hover:bg-white/[0.05] hover:border-white/10 hover:-translate-y-0.5 flex flex-col gap-2">
          <span className="text-[0.75rem] uppercase tracking-[0.05em] text-zinc-500 font-semibold">
            {t.about.github}
          </span>
          <button
            type="button"
            onClick={() => openLink('https://github.com/fonlan/resh')}
            className="flex items-center gap-2 text-zinc-100 bg-transparent border-0 p-0 m-0 cursor-pointer text-base font-medium transition-opacity duration-200 hover:opacity-80 hover:underline font-inherit"
          >
            <Github size={16} />
            <span>fonlan/resh</span>
            <ExternalLink size={12} className="opacity-50" />
          </button>
        </div>

        <div className="bg-white/[0.03] border border-white/[0.05] p-5 rounded-2xl transition-all duration-200 hover:bg-white/[0.05] hover:border-white/10 hover:-translate-y-0.5 flex flex-col gap-2">
          <span className="text-[0.75rem] uppercase tracking-[0.05em] text-zinc-500 font-semibold">
            {t.about.license}
          </span>
          <div className="text-base text-zinc-100 font-medium flex items-center min-h-6 gap-2">
            <Shield size={16} className="text-blue-400" />
            <span>MIT License</span>
          </div>
        </div>

        <div className="bg-white/[0.03] border border-white/[0.05] p-5 rounded-2xl transition-all duration-200 hover:bg-white/[0.05] hover:border-white/10 hover:-translate-y-0.5 flex flex-col gap-2">
          <span className="text-[0.75rem] uppercase tracking-[0.05em] text-zinc-500 font-semibold">
            {t.about.techStack}
          </span>
          <div className="text-base text-zinc-100 font-medium flex items-center min-h-6 gap-2">
            <Cpu size={16} className="text-purple-400" />
            <span className="text-sm">Modern Stack</span>
          </div>
        </div>
      </div>

      <div className="flex flex-wrap justify-center gap-3 mt-4 w-full max-w-[600px]">
        {techStack.map(tech => (
          <span key={tech} className="bg-white/[0.05] text-zinc-300 px-3 py-1.5 rounded-lg text-sm border border-white/[0.05]">
            {tech}
          </span>
        ))}
      </div>

      <div className="mt-auto pt-12 text-center text-zinc-600 text-sm max-w-[400px] animate-[fadeIn_0.6s_cubic-bezier(0.22,1,0.36,1)_0.3s_forwards] opacity-0" style={{ animationFillMode: 'forwards' }}>
        <div className="flex items-center justify-center gap-2 mb-2">
          <Heart size={14} className="text-red-500 fill-red-500" />
          <span>Made with passion by fonlan</span>
        </div>
        <p>{t.about.thanks}</p>
      </div>
    </div>
  );
};
