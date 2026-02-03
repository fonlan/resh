import React from 'react';
import { Terminal, Github, Heart, Shield, Cpu, ExternalLink } from 'lucide-react';
import { useTranslation } from '../../i18n';
import { invoke } from '@tauri-apps/api/core';
import './AboutTab.css';

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
    <div className="about-tab">
      <div className="about-header">
        <div className="about-logo-wrapper">
          <Terminal size={48} className="about-logo-icon" />
        </div>
        <h1 className="about-title">Resh</h1>
        <div className="about-version">{t.about.version} 0.1.0</div>
      </div>

      <p className="about-description">
        {t.about.description}
      </p>

      <div className="about-grid">
        <div className="about-card">
          <span className="about-card-label">{t.about.author}</span>
          <div className="about-card-value">
            <span className="font-bold">fonlan</span>
          </div>
        </div>

        <div className="about-card">
          <span className="about-card-label">{t.about.github}</span>
          <button 
            type="button"
            onClick={() => openLink('https://github.com/fonlan/resh')}
            className="about-link-btn"
          >
            <Github size={16} />
            <span>fonlan/resh</span>
            <ExternalLink size={12} className="opacity-50" />
          </button>
        </div>

        <div className="about-card">
          <span className="about-card-label">{t.about.license}</span>
          <div className="about-card-value flex items-center gap-2">
            <Shield size={16} className="text-blue-400" />
            <span>MIT License</span>
          </div>
        </div>

        <div className="about-card">
          <span className="about-card-label">{t.about.techStack}</span>
          <div className="about-card-value flex items-center gap-2">
            <Cpu size={16} className="text-purple-400" />
            <span className="text-sm">Modern Stack</span>
          </div>
        </div>
      </div>

      <div className="tech-stack-container">
        {techStack.map(tech => (
          <span key={tech} className="tech-badge">{tech}</span>
        ))}
      </div>

      <div className="about-footer">
        <div className="flex items-center justify-center gap-2 mb-2">
          <Heart size={14} className="text-red-500 fill-red-500" />
          <span>Made with passion by fonlan</span>
        </div>
        <p>{t.about.thanks}</p>
      </div>
    </div>
  );
};
