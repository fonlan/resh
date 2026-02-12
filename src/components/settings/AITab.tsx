import React, { useState, useRef, useEffect } from 'react';
import { createPortal } from 'react-dom';
import { Plus, Edit2, Trash2, Copy, Check, ExternalLink, Loader2 } from 'lucide-react';
import { AIChannel, AIModel, ProxyConfig, GeneralSettings } from '../../types';
import { generateId } from '../../utils/idGenerator';
import { FormModal } from '../FormModal';
import { ConfirmationModal } from '../ConfirmationModal';
import { CustomSelect } from '../CustomSelect';
import { useTranslation } from '../../i18n';
import { invoke } from '@tauri-apps/api/core';

interface AITabProps {
  aiChannels: AIChannel[];
  aiModels: AIModel[];
  proxies: ProxyConfig[];
  general: GeneralSettings;
  additionalPrompt?: string | null;
  onAIChannelsUpdate: (channels: AIChannel[]) => void;
  onAIModelsUpdate: (models: AIModel[]) => void;
  onGeneralUpdate: (settings: GeneralSettings) => void;
  onAdditionalPromptUpdate: (prompt: string | null | undefined) => void;
}

interface DeviceCodeResponse {
  device_code: string;
  user_code: string;
  verification_uri: string;
  expires_in: number;
  interval: number;
}

export const AITab: React.FC<AITabProps> = ({
  aiChannels,
  aiModels,
  proxies,
  general,
  additionalPrompt,
  onAIChannelsUpdate,
  onAIModelsUpdate,
  onGeneralUpdate,
  onAdditionalPromptUpdate,
}) => {
  const { t } = useTranslation();
  // State for Channel Form
  const [isChannelFormOpen, setIsChannelFormOpen] = useState(false);
  const [editingChannel, setEditingChannel] = useState<AIChannel | null>(null);
  const [channelFormData, setChannelFormData] = useState<Partial<AIChannel>>({});
  const [channelToDelete, setChannelToDelete] = useState<string | null>(null);

  // Copilot Auth State
  const [copilotAuthData, setCopilotAuthData] = useState<DeviceCodeResponse | null>(null);
  const [isAuthLoading, setIsAuthLoading] = useState(false);
  const [isPolling, setIsPolling] = useState(false);
  const [authError, setAuthError] = useState<string | null>(null);

  // State for Model Form
  const [isModelFormOpen, setIsModelFormOpen] = useState(false);
  const [editingModel, setEditingModel] = useState<AIModel | null>(null);
  const [modelFormData, setModelFormData] = useState<Partial<AIModel>>({});
  const [modelToDelete, setModelToDelete] = useState<string | null>(null);
  const [fetchedModels, setFetchedModels] = useState<string[]>([]);
  const [isFetchingModels, setIsFetchingModels] = useState(false);
  const [showModelSuggestions, setShowModelSuggestions] = useState(false);
  const [dropdownPosition, setDropdownPosition] = useState<{ top: number, left: number, width: number } | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  // Sort channels and models by name
  const sortedChannels = React.useMemo(() => {
    return [...aiChannels].sort((a, b) => a.name.localeCompare(b.name));
  }, [aiChannels]);

  const sortedModels = React.useMemo(() => {
    return [...aiModels].sort((a, b) => a.name.localeCompare(b.name));
  }, [aiModels]);

  // Update position when showing suggestions
  useEffect(() => {
    if (showModelSuggestions && inputRef.current) {
        const updatePos = () => {
            if (inputRef.current) {
                const rect = inputRef.current.getBoundingClientRect();
                setDropdownPosition({
                    top: rect.bottom,
                    left: rect.left,
                    width: rect.width
                });
            }
        };
        updatePos();
        // Listen to both window resize and scroll (capturing phase to catch modal scroll)
        window.addEventListener('resize', updatePos);
        window.addEventListener('scroll', updatePos, true);
        
        return () => {
            window.removeEventListener('resize', updatePos);
            window.removeEventListener('scroll', updatePos, true);
        };
    }
  }, [showModelSuggestions]);

  // Reset fetched models when channel changes
  React.useEffect(() => {
    setFetchedModels([]);
  }, [modelFormData.channelId]);

  const handleFetchModels = async () => {
    if (!modelFormData.channelId || fetchedModels.length > 0 || isFetchingModels) return;
    
    setIsFetchingModels(true);
    try {
      const models = await invoke<string[]>('fetch_ai_models', { channelId: modelFormData.channelId });
      setFetchedModels(models);
    } catch (e) {
      // Failed to fetch models
    } finally {
      setIsFetchingModels(false);
    }
  };

  // --- Copilot Handlers ---
  const startCopilotAuth = async () => {
    setIsAuthLoading(true);
    setAuthError(null);
    try {
      const data = await invoke<DeviceCodeResponse>('start_copilot_auth');
      setCopilotAuthData(data);
      pollCopilotAuth(data);
    } catch (e: any) {
      setAuthError(e.toString());
    } finally {
      setIsAuthLoading(false);
    }
  };

  const pollCopilotAuth = async (data: DeviceCodeResponse) => {
    setIsPolling(true);
    let attempts = 0;
    const maxAttempts = 100; // Safety break
    
    // We need a way to stop polling if the modal closes. 
    // Since we are inside a function closure, we can't easily check a ref that changes.
    // Ideally we should use a useEffect or a ref for cancellation.
    // For now, valid as long as isChannelFormOpen is true? 
    // Actually, `channelFormData.type` should be checked.

    const pollLoop = async () => {
      if (attempts >= maxAttempts) {
        setIsPolling(false);
        setAuthError("Auth timed out");
        return;
      }

      try {
        const token = await invoke<string>('poll_copilot_auth', { deviceCode: data.device_code });
        // Success!
        setChannelFormData(prev => ({ ...prev, apiKey: token, isActive: true }));
        setCopilotAuthData(null);
        setIsPolling(false);
      } catch (e: any) {
        const err = e.toString();
        if (err.includes("pending") || err.includes("slow_down")) {
           // Continue polling
           attempts++;
           setTimeout(pollLoop, (data.interval + 1) * 1000);
        } else {
           setIsPolling(false);
           setAuthError(err);
        }
      }
    };
    
    setTimeout(pollLoop, (data.interval + 1) * 1000);
  };

  const copyUserCode = () => {
    if (copilotAuthData) {
      navigator.clipboard.writeText(copilotAuthData.user_code);
    }
  };

  // --- Channel Handlers ---

  const handleAddChannel = () => {
    setEditingChannel(null);
      setChannelFormData({
      name: '',
      type: 'openai',
      endpoint: '',
      apiKey: '',
      proxyId: null,
      isActive: true,
      synced: true,
    });

    setCopilotAuthData(null);
    setIsPolling(false);
    setAuthError(null);
    setIsChannelFormOpen(true);
  };

  const handleEditChannel = (channel: AIChannel) => {
    setEditingChannel(channel);
    setChannelFormData({ ...channel });
    setCopilotAuthData(null);
    setIsPolling(false);
    setAuthError(null);
    setIsChannelFormOpen(true);
  };

  const handleDeleteChannel = (id: string) => {
    setChannelToDelete(id);
  };

  const confirmDeleteChannel = () => {
    if (channelToDelete) {
      onAIChannelsUpdate(aiChannels.filter((c) => c.id !== channelToDelete));
      onAIModelsUpdate(aiModels.filter((m) => m.channelId !== channelToDelete));
      setChannelToDelete(null);
    }
  };

  const handleSaveChannel = () => {
    if (!channelFormData.name) return; // Simple validation

    const now = new Date().toISOString();

    // Determine active state: Copilot channels must be authenticated (have apiKey) to be active
    const type = (channelFormData.type as 'openai' | 'copilot') || 'openai';
    const apiKey = channelFormData.apiKey;
    let isActive = channelFormData.isActive !== undefined ? channelFormData.isActive : true;

    if (type === 'copilot' && !apiKey) {
      isActive = false;
    }

    if (editingChannel) {
      const updatedChannel: AIChannel = {
        ...editingChannel,
        name: channelFormData.name || 'New Channel',
        type,
        endpoint: channelFormData.endpoint,
        apiKey: channelFormData.apiKey,
        proxyId: channelFormData.proxyId,
        isActive,
        synced: channelFormData.synced !== undefined ? channelFormData.synced : editingChannel.synced,
        updatedAt: now,
      };
      onAIChannelsUpdate(aiChannels.map((c) => (c.id === editingChannel.id ? updatedChannel : c)));
    } else {
      const newChannel: AIChannel = {
        id: generateId(),
        name: channelFormData.name || 'New Channel',
        type,
        endpoint: channelFormData.endpoint,
        apiKey: channelFormData.apiKey,
        proxyId: channelFormData.proxyId,
        isActive,
        synced: channelFormData.synced !== undefined ? channelFormData.synced : true,
        updatedAt: now,
      };
      onAIChannelsUpdate([...aiChannels, newChannel]);
    }
    setIsChannelFormOpen(false);
  };

  // --- Model Handlers ---

  const handleAddModel = () => {
    if (aiChannels.length === 0) {
      alert('Please create an AI Channel first.');
      return;
    }
    setEditingModel(null);
    setModelFormData({
      name: '',
      channelId: aiChannels[0].id,
      enabled: true,
      synced: true,
    });
    setIsModelFormOpen(true);
  };

  const handleEditModel = (model: AIModel) => {
    setEditingModel(model);
    setModelFormData({ ...model });
    setIsModelFormOpen(true);
  };

  const handleDeleteModel = (id: string) => {
    setModelToDelete(id);
  };

  const confirmDeleteModel = () => {
    if (modelToDelete) {
      onAIModelsUpdate(aiModels.filter((m) => m.id !== modelToDelete));
      setModelToDelete(null);
    }
  };

  const handleSaveModel = () => {
    if (!modelFormData.name || !modelFormData.channelId) return;

    const now = new Date().toISOString();

    if (editingModel) {
      const updatedModel: AIModel = {
        ...editingModel,
        name: modelFormData.name,
        channelId: modelFormData.channelId,
        enabled: modelFormData.enabled ?? true,
        synced: modelFormData.synced ?? true,
        updatedAt: now,
      };
      onAIModelsUpdate(aiModels.map((m) => (m.id === editingModel.id ? updatedModel : m)));
    } else {
      const newModel: AIModel = {
        id: generateId(),
        name: modelFormData.name,
        channelId: modelFormData.channelId,
        enabled: modelFormData.enabled ?? true,
        synced: modelFormData.synced ?? true,
        updatedAt: now,
      };
      onAIModelsUpdate([...aiModels, newModel]);
    }
    setIsModelFormOpen(false);
  };

  return (
    <div className="w-full max-w-full">
      {/* General Settings Section */}
      <div className="flex justify-between items-center mb-4">
        <h3 className="text-base font-semibold ">{t.ai.configuration}</h3>
      </div>

      <div className="mb-8">
         <div className="flex gap-4">
            <div className="flex flex-col gap-1.5 mb-4 flex-1">
               <label htmlFor="ai-max-history" className="block text-sm font-medium text-zinc-400 mb-1.5 ">{t.ai.maxChatContext}</label>
               <input
id="ai-max-history"
                    type="number"
                    className="w-full px-3 py-2 text-sm border border-zinc-700/50 rounded-md outline-none transition-all focus:border-blue-500 focus:shadow-[0_0_20px_rgba(59,130,246,0.2)] disabled:opacity-50 disabled:cursor-not-allowed bg-[var(--bg-primary)] text-[var(--text-primary)] placeholder:text-[var(--text-muted)]"
                   value={general.aiMaxHistory || 10}
                   onChange={(e) => {
                       const val = parseInt(e.target.value);
                       if (!isNaN(val)) {
                           onGeneralUpdate({...general, aiMaxHistory: val});
                       }
                   }}
                   min={1}
                   max={100}
               />
            </div>
            <div className="flex flex-col gap-1.5 mb-4 flex-1">
               <label htmlFor="ai-timeout" className="block text-sm font-medium text-zinc-400 mb-1.5 ">{t.ai.requestTimeout}</label>
               <input
id="ai-timeout"
                    type="number"
                    className="w-full px-3 py-2 text-sm border border-zinc-700/50 rounded-md outline-none transition-all focus:border-blue-500 focus:shadow-[0_0_20px_rgba(59,130,246,0.2)] disabled:opacity-50 disabled:cursor-not-allowed bg-[var(--bg-primary)] text-[var(--text-primary)] placeholder:text-[var(--text-muted)]"
                   value={general.aiTimeout || 120}
                   onChange={(e) => {
                       const val = parseInt(e.target.value);
                       if (!isNaN(val)) {
                           onGeneralUpdate({...general, aiTimeout: val});
                       }
                   }}
                   min={1}
                   max={600}
               />
            </div>
          </div>
       </div>

       <div className="mb-8">
          <div className="flex flex-col gap-1.5 mb-4">
             <label htmlFor="ai-additional-prompt" className="block text-sm font-medium text-zinc-400 mb-1.5 ">{t.ai.globalAdditionalPrompt}</label>
             <div className="text-xs text-zinc-400 mb-2">
                 {t.ai.globalAdditionalPromptDesc}
             </div>
              <textarea
                  id="ai-additional-prompt"
                  className="w-full h-32 py-2 px-3 text-sm border border-zinc-700/50 rounded-md outline-none transition-all focus:border-blue-500 focus:shadow-[0_0_20px_rgba(59,130,246,0.2)] disabled:opacity-50 disabled:cursor-not-allowed bg-[var(--bg-primary)] text-[var(--text-primary)] placeholder:text-[var(--text-muted)] font-sans resize-y min-h-[100px]"
                  value={additionalPrompt || ''}
                  onChange={(e) => onAdditionalPromptUpdate(e.target.value)}
                  placeholder={t.ai.globalAdditionalPromptPlaceholder}
              />
          </div>
       </div>

       {/* AI Channels Section */}
      <div className="flex justify-between items-center mb-4">
        <h3 className="text-base font-semibold ">{t.ai.channels}</h3>
        <button type="button" onClick={handleAddChannel} className="inline-flex items-center justify-center gap-2 px-4 py-2 text-sm font-medium rounded bg-blue-500 text-white shadow-[0_0_20px_rgba(59,130,246,0.2)] border-none cursor-pointer transition-all whitespace-nowrap hover:brightness-110 hover:-translate-y-px active:translate-y-0 font-sans">
          <Plus size={16} />
          {t.ai.addChannel}
        </button>
      </div>

      <div className="flex flex-col gap-2 mb-8">
        {sortedChannels.length === 0 ? (
          <div className="flex flex-col items-center justify-center p-12 text-center bg-[var(--bg-primary)] border-[1.5px] border-dashed border-zinc-700/50 rounded-md">
            <p className="text-sm text-[var(--text-muted)] m-0">{t.ai.noChannels}</p>
          </div>
        ) : (
          sortedChannels.map((channel) => (
            <div key={channel.id} className={`flex items-center justify-between p-3 bg-[var(--bg-primary)] border-[1.5px] border-zinc-700/50 rounded-md transition-all gap-3 ${!channel.isActive ? 'opacity-60' : ''} hover:border-blue-500 hover:shadow-[0_0_20px_rgba(59,130,246,0.2)] hover:-translate-y-px`}>
              <div className="flex flex-col gap-1 flex-1 min-w-0">
                <div className="flex items-center gap-2">
                  <span className="text-sm font-semibold text-[var(--text-primary)] m-0 whitespace-nowrap overflow-hidden text-overflow-ellipsis">{channel.name}</span>
                  {!channel.isActive && <span className="inline-flex items-center px-2 py-0.5 text-xs font-medium bg-[var(--bg-primary)] text-[var(--text-muted)] rounded border border-zinc-700/50 whitespace-nowrap">{t.ai.disabled}</span>}
                </div>
                <p className="text-xs text-[var(--text-muted)] m-0 whitespace-nowrap overflow-hidden text-overflow-ellipsis">Type: {channel.type}</p>
              </div>
              <div className="flex items-center gap-1.5 flex-shrink-0">
                <button
                  type="button"
                  onClick={() => handleEditChannel(channel)}
                  className="inline-flex items-center justify-center w-8 h-8 p-0 bg-[var(--bg-primary)] text-[var(--text-secondary)] border border-zinc-700/50 rounded cursor-pointer transition-all hover:bg-[var(--bg-elevated)] hover:text-[var(--text-primary)] hover:border-blue-500"
                  title={t.common.edit}
                >
                  <Edit2 size={14} />
                </button>
                <button
                  type="button"
                  onClick={() => handleDeleteChannel(channel.id)}
                  className="inline-flex items-center justify-center w-8 h-8 p-0 bg-[var(--bg-primary)] text-[var(--text-secondary)] border border-zinc-700/50 rounded cursor-pointer transition-all hover:bg-red-500/10 hover:border-red-500 hover:text-red-500"
                  title={t.common.delete}
                >
                  <Trash2 size={14} />
                </button>
              </div>
            </div>
          ))
        )}
      </div>

      {/* AI Models Section */}
      <div className="flex justify-between items-center mb-4 mt-8">
        <h3 className="text-base font-semibold ">{t.ai.models}</h3>
        <button type="button" onClick={handleAddModel} className="inline-flex items-center justify-center gap-2 px-4 py-2 text-sm font-medium rounded bg-blue-500 text-white shadow-[0_0_20px_rgba(59,130,246,0.2)] border-none cursor-pointer transition-all whitespace-nowrap hover:brightness-110 hover:-translate-y-px active:translate-y-0 font-sans">
          <Plus size={16} />
          {t.ai.addModel}
        </button>
      </div>

      <div className="flex flex-col gap-2">
        {sortedModels.length === 0 ? (
          <div className="flex flex-col items-center justify-center p-12 text-center bg-[var(--bg-primary)] border-[1.5px] border-dashed border-zinc-700/50 rounded-md">
            <p className="text-sm text-[var(--text-muted)] m-0">{t.ai.noModels}</p>
          </div>
        ) : (
          sortedModels.map((model) => {
            const channel = aiChannels.find(c => c.id === model.channelId);
            const isChannelActive = channel ? channel.isActive : false;
            const effectivelyEnabled = model.enabled && isChannelActive;

            return (
              <div key={model.id} className={`flex items-center justify-between p-3 bg-[var(--bg-primary)] border-[1.5px] border-zinc-700/50 rounded-md transition-all gap-3 ${!effectivelyEnabled ? 'opacity-60' : ''} hover:border-blue-500 hover:shadow-[0_0_20px_rgba(59,130,246,0.2)] hover:-translate-y-px`}>
                <div className="flex flex-col gap-1 flex-1 min-w-0">
                  <div className="flex items-center gap-2">
                    <span className="text-sm font-semibold text-[var(--text-primary)] m-0 whitespace-nowrap overflow-hidden text-overflow-ellipsis">{model.name}</span>
                    {!model.enabled && <span className="inline-flex items-center px-2 py-0.5 text-xs font-medium bg-[var(--bg-primary)] text-[var(--text-muted)] rounded border border-zinc-700/50 whitespace-nowrap">{t.ai.disabled}</span>}
                    {model.enabled && !isChannelActive && (
                      <span className="inline-flex items-center px-2 py-0.5 text-xs font-medium bg-yellow-900/50 text-yellow-200 rounded border border-zinc-700/50 whitespace-nowrap" title="Parent channel is disabled">
                        {t.ai.channelDisabled}
                      </span>
                    )}
                  </div>
                  <p className="text-xs text-[var(--text-muted)] m-0 whitespace-nowrap overflow-hidden text-overflow-ellipsis">Channel: {channel ? channel.name : 'Unknown Channel'}</p>
                </div>
                <div className="flex items-center gap-1.5 flex-shrink-0">
                  <button
                    type="button"
                    onClick={() => handleEditModel(model)}
                    className="inline-flex items-center justify-center w-8 h-8 p-0 bg-[var(--bg-primary)] text-[var(--text-secondary)] border border-zinc-700/50 rounded cursor-pointer transition-all hover:bg-[var(--bg-elevated)] hover:text-[var(--text-primary)] hover:border-blue-500"
                    title={t.common.edit}
                  >
                    <Edit2 size={14} />
                  </button>
                  <button
                    type="button"
                    onClick={() => handleDeleteModel(model.id)}
                    className="inline-flex items-center justify-center w-8 h-8 p-0 bg-[var(--bg-primary)] text-[var(--text-secondary)] border border-zinc-700/50 rounded cursor-pointer transition-all hover:bg-red-500/10 hover:border-red-500 hover:text-red-500"
                    title={t.common.delete}
                  >
                    <Trash2 size={14} />
                  </button>
                </div>
              </div>
            );
          })
        )}
      </div>

      {/* Channel Form Modal */}
      <FormModal
        isOpen={isChannelFormOpen}
        title={editingChannel ? t.ai.editChannel : t.ai.addChannel}
        onClose={() => setIsChannelFormOpen(false)}
        onSubmit={handleSaveChannel}
        extraFooterContent={
          <div className="flex items-center gap-2 mr-auto">
            <input
              type="checkbox"
              id="channel-synced"
              checked={channelFormData.synced ?? true}
              onChange={(e) => setChannelFormData({ ...channelFormData, synced: e.target.checked })}
              className="appearance-none -webkit-appearance-none w-[18px] h-[18px] border-[1.5px] border-zinc-700/50 rounded bg-[var(--bg-primary)] cursor-pointer relative transition-all flex-shrink-0 inline-flex items-center justify-center vertical-middle checked:bg-blue-500 checked:border-blue-500 checked:shadow-[0_0_20px_rgba(59,130,246,0.2)] hover:border-blue-500 focus:outline-none focus:shadow-[0_0_0_3px_rgba(59,130,246,0.2)] disabled:opacity-50 disabled:cursor-not-allowed"
            />
            <label htmlFor="channel-synced" className="text-sm font-medium text-zinc-300 cursor-pointer">
              {t.common.syncThisItem || 'Sync this item'}
            </label>
          </div>
        }
      >
        <div className="flex items-center gap-2 mb-4">
          <input
            type="checkbox"
            id="channel-active"
            className="appearance-none -webkit-appearance-none w-[18px] h-[18px] border-[1.5px] border-zinc-700/50 rounded bg-[var(--bg-primary)] cursor-pointer relative transition-all flex-shrink-0 inline-flex items-center justify-center vertical-middle checked:bg-blue-500 checked:border-blue-500 checked:shadow-[0_0_20px_rgba(59,130,246,0.2)] hover:border-blue-500 focus:outline-none focus:shadow-[0_0_0_3px_rgba(59,130,246,0.2)] disabled:opacity-50 disabled:cursor-not-allowed"
            checked={channelFormData.isActive ?? true}
            onChange={(e) => setChannelFormData({ ...channelFormData, isActive: e.target.checked })}
            disabled={channelFormData.type === 'copilot' && !channelFormData.apiKey}
          />
          <label htmlFor="channel-active" className="text-sm cursor-pointer">{t.ai.channelForm.active}</label>
        </div>

        <div className="flex flex-col gap-1.5 mb-4">
          <label htmlFor="channel-name" className="block text-sm font-medium text-zinc-400 mb-1.5 ">{t.ai.channelForm.name}</label>
          <input
            id="channel-name"
            type="text"
            className="w-full px-3 py-2 text-sm border border-zinc-700/50 rounded-md outline-none transition-all focus:border-blue-500 focus:shadow-[0_0_20px_rgba(59,130,246,0.2)] disabled:opacity-50 disabled:cursor-not-allowed bg-[var(--bg-primary)] text-[var(--text-primary)] placeholder:text-[var(--text-muted)]"
            value={channelFormData.name || ''}
            onChange={(e) => setChannelFormData({ ...channelFormData, name: e.target.value })}
            placeholder={t.ai.channelForm.namePlaceholder}
          />
        </div>
        <div className="flex flex-col gap-1.5 mb-4">
          <label htmlFor="channel-type" className="block text-sm font-medium text-zinc-400 mb-1.5 ">{t.ai.channelForm.type}</label>
          <CustomSelect
            id="channel-type"
            value={channelFormData.type || 'openai'}
            onChange={(val) => {
              const newType = val as any;
              setChannelFormData({ 
                ...channelFormData, 
                type: newType,
                apiKey: '', // Clear API key on type change
                isActive: newType === 'copilot' ? false : (channelFormData.isActive ?? true)
              });
              // Reset auth state when switching types
              setCopilotAuthData(null);
              setIsPolling(false);
            }}
            options={[
                { value: 'openai', label: 'OpenAI' },
                { value: 'copilot', label: 'GitHub Copilot' }
            ]}
          />
        </div>

{channelFormData.type === 'copilot' ? (
           <div className="flex flex-col gap-1.5 mb-4 p-4 bg-[var(--bg-primary)] rounded-lg border border-zinc-700/50">
              <label className="block text-sm font-medium text-zinc-400 mb-2 ">GitHub Authentication</label>

             {channelFormData.apiKey ? (
               <div className="flex items-center gap-2 text-green-400 mb-4">
                 <Check size={18} />
                 <span>Authenticated with GitHub</span>
                 <button
                   type="button"
                   onClick={() => setChannelFormData({...channelFormData, apiKey: ''})}
                   className="text-xs text-zinc-400 hover:text-white underline ml-2 bg-transparent border-none cursor-pointer p-0"
                 >
                   Reset
                 </button>
               </div>
             ) : (
               <>
                 {!copilotAuthData && (
                   <button
                     type="button"
                     onClick={startCopilotAuth}
disabled={isAuthLoading}
                      className="inline-flex items-center justify-center w-full gap-2 px-4 py-2 text-sm font-medium rounded bg-[var(--bg-primary)] text-[var(--text-primary)] border border-zinc-700/50 cursor-pointer transition-all whitespace-nowrap hover:bg-[var(--bg-elevated)] hover:border-blue-500 font-sans disabled:opacity-50 disabled:cursor-not-allowed"
                   >
                     {isAuthLoading ? <Loader2 size={16} className="animate-spin" /> : null}
                     Sign in with GitHub
                   </button>
                 )}

                 {authError && (
                   <div className="text-red-500 text-sm mt-2">{authError}</div>
                 )}

                 {copilotAuthData && (
                   <div className="mt-4 space-y-4">
                     <div className="text-sm text-zinc-300">
                       1. Copy code: <strong className="text-white select-all">{copilotAuthData.user_code}</strong>
                     </div>
                      <div className="text-sm text-zinc-300">
                        2. Open link:
                        <button
                          type="button"
                          onClick={() => invoke('open_url', { url: copilotAuthData.verification_uri })}
                          className="text-blue-400 hover:underline ml-1 inline-flex items-center gap-1 bg-transparent border-none p-0 cursor-pointer"
                        >
                          {copilotAuthData.verification_uri} <ExternalLink size={12} />
                        </button>
                      </div>


                     <div className="flex gap-2">
                        <button
                          type="button"
                          onClick={copyUserCode}
                          className="inline-flex items-center justify-center flex-1 gap-2 px-4 py-2 text-sm font-medium rounded bg-zinc-800 text-zinc-100 border-[1.5px] border-zinc-700/50 cursor-pointer transition-all whitespace-nowrap hover:bg-zinc-700 hover:border-blue-500 font-sans"
                        >
                          <Copy size={14} /> Copy Code
                        </button>
                     </div>

                     <div className="text-center text-xs text-zinc-400 flex items-center justify-center gap-2">
                       {isPolling && <Loader2 size={12} className="animate-spin" />}
                       Waiting for authentication...
                     </div>
                   </div>
                 )}
               </>
             )}
           </div>
        ) : (
          <>
            <div className="flex flex-col gap-1.5 mb-4">
              <label htmlFor="channel-endpoint" className="block text-sm font-medium text-zinc-400 mb-1.5 ">{t.ai.channelForm.endpoint}</label>
              <input
                id="channel-endpoint"
                type="text"
                className="w-full px-3 py-2 text-sm border border-zinc-700/50 rounded-md outline-none transition-all focus:border-blue-500 focus:shadow-[0_0_20px_rgba(59,130,246,0.2)] disabled:opacity-50 disabled:cursor-not-allowed bg-[var(--bg-primary)] text-[var(--text-primary)] placeholder:text-[var(--text-muted)]"
                value={channelFormData.endpoint || ''}
                onChange={(e) => setChannelFormData({ ...channelFormData, endpoint: e.target.value })}
                placeholder="https://api.openai.com/v1"
              />
            </div>
            <div className="flex flex-col gap-1.5 mb-4">
              <label htmlFor="channel-apikey" className="block text-sm font-medium text-zinc-400 mb-1.5 ">{t.ai.channelForm.apiKey}</label>
              <input
                id="channel-apikey"
                type="password"
                className="w-full px-3 py-2 text-sm border border-zinc-700/50 rounded-md outline-none transition-all focus:border-blue-500 focus:shadow-[0_0_20px_rgba(59,130,246,0.2)] disabled:opacity-50 disabled:cursor-not-allowed bg-[var(--bg-primary)] text-[var(--text-primary)] placeholder:text-[var(--text-muted)]"
                value={channelFormData.apiKey || ''}
                onChange={(e) => setChannelFormData({ ...channelFormData, apiKey: e.target.value })}
                placeholder="sk-..."
              />
            </div>
          </>
        )}

        <div className="flex flex-col gap-1.5 mb-4 mt-4 pt-4 border-t border-zinc-700/50">
          <label htmlFor="channel-proxy" className="block text-sm font-medium text-zinc-400 mb-1.5 ">{t.common.proxy}</label>
          <CustomSelect
            id="channel-proxy"
            value={channelFormData.proxyId || ''}
            onChange={(val) => setChannelFormData({ ...channelFormData, proxyId: val === '' ? null : val })}
            options={[
                { value: '', label: t.common.noProxy },
                ...proxies.map(proxy => ({
                    value: proxy.id,
                    label: `${proxy.name} (${proxy.host}:${proxy.port})`
                }))
            ]}
          />
        </div>
      </FormModal>

      {/* Model Form Modal */}
      <FormModal
        isOpen={isModelFormOpen}
        title={editingModel ? t.ai.editModel : t.ai.addModel}
        onClose={() => setIsModelFormOpen(false)}
        onSubmit={handleSaveModel}
        extraFooterContent={
          <div className="flex items-center gap-2 mr-auto">
            <input
              type="checkbox"
              id="model-synced"
              checked={modelFormData.synced ?? true}
              onChange={(e) => setModelFormData({ ...modelFormData, synced: e.target.checked })}
              className="appearance-none -webkit-appearance-none w-[18px] h-[18px] border-[1.5px] border-zinc-700/50 rounded bg-[var(--bg-primary)] cursor-pointer relative transition-all flex-shrink-0 inline-flex items-center justify-center vertical-middle checked:bg-blue-500 checked:border-blue-500 checked:shadow-[0_0_20px_rgba(59,130,246,0.2)] hover:border-blue-500 focus:outline-none focus:shadow-[0_0_0_3px_rgba(59,130,246,0.2)] disabled:opacity-50 disabled:cursor-not-allowed"
            />
            <label htmlFor="model-synced" className="text-sm font-medium text-zinc-300 cursor-pointer">
              {t.common.syncThisItem || 'Sync this item'}
            </label>
          </div>
        }
      >
          <div className="flex flex-col gap-1.5 mb-4 relative">
            <label htmlFor="model-name" className="block text-sm font-medium text-zinc-400 mb-1.5 ">{t.ai.modelForm.name}</label>
             <div className="relative">
               <input
                 id="model-name"
                 ref={inputRef}
                 type="text"
                 className="w-full px-3 py-2 pr-8 text-sm text-zinc-100 bg-zinc-900 border border-zinc-700/50 rounded-md outline-none transition-all focus:border-blue-500 focus:shadow-[0_0_20px_rgba(59,130,246,0.2)] placeholder:text-zinc-500 disabled:opacity-50 disabled:cursor-not-allowed disabled:bg-zinc-800"
                  value={modelFormData.name || ''}
                 onChange={(e) => setModelFormData({ ...modelFormData, name: e.target.value })}
                 placeholder={t.ai.modelForm.namePlaceholder}
                 onFocus={() => {
                    handleFetchModels();
                    setShowModelSuggestions(true);
                 }}
                 onBlur={() => setTimeout(() => setShowModelSuggestions(false), 200)}
                 autoComplete="off"
               />
               {isFetchingModels && (
                 <div className="absolute right-2 top-1/2 transform -translate-y-1/2 pointer-events-none">
                   <Loader2 size={16} className="animate-spin text-zinc-400" />
                 </div>
               )}

{showModelSuggestions && fetchedModels.length > 0 && dropdownPosition && createPortal(
                  <ul
                    className="flex flex-wrap gap-1.5 p-2 bg-[var(--bg-primary)] border-[1.5px] border-zinc-700/50 rounded mt-1 max-w-[300px] z-[1000] fixed"
                   style={{
                       top: dropdownPosition.top,
                       left: dropdownPosition.left,
                       width: dropdownPosition.width,
                       marginTop: '4px',
                       maxHeight: '200px',
                       zIndex: 9999
                   }}
                 >
                   {fetchedModels
                     .filter(m => !modelFormData.name || m.toLowerCase().includes(modelFormData.name.toLowerCase()))
                     .map(model => (
<li
                        key={model}
                        className="p-1 px-2.5 text-xs bg-[var(--bg-primary)] text-[var(--text-muted)] border border-zinc-700/50 rounded cursor-pointer transition-all hover:bg-[var(--bg-elevated)] hover:text-[var(--text-primary)] hover:border-blue-500"
                       onMouseDown={(e) => {
                         e.preventDefault();
                         setModelFormData({ ...modelFormData, name: model });
                         setShowModelSuggestions(false);
                       }}
                     >
                       {model}
                     </li>
                   ))}
{fetchedModels.filter(m => !modelFormData.name || m.toLowerCase().includes(modelFormData.name.toLowerCase())).length === 0 && (
                       <li className="p-1 px-2.5 text-xs bg-[var(--bg-primary)] text-[var(--text-muted)] border border-zinc-700/50 rounded cursor-pointer transition-all hover:bg-[var(--bg-elevated)] hover:text-[var(--text-primary)] hover:border-blue-500" style={{ cursor: 'default', opacity: 0.5 }}>
                         No matches found
                      </li>
                   )}
                 </ul>,
                 document.body
               )}
             </div>
           </div>
         <div className="flex flex-col gap-1.5 mb-4">
           <label htmlFor="model-channel" className="block text-sm font-medium text-zinc-400 mb-1.5 ">{t.ai.modelForm.channel}</label>
          <CustomSelect
            id="model-channel"
            value={modelFormData.channelId || ''}
            onChange={(val) => setModelFormData({ ...modelFormData, channelId: val })}
            options={aiChannels.map(channel => ({
                value: channel.id,
                label: channel.name
            }))}
          />
        </div>

         <div className="flex flex-col gap-1.5 mb-4 flex items-center gap-2 mt-4">
           <input
             type="checkbox"
             id="model-active"
             className="appearance-none -webkit-appearance-none w-[18px] h-[18px] border-[1.5px] border-zinc-700/50 rounded bg-zinc-900 cursor-pointer relative transition-all flex-shrink-0 inline-flex items-center justify-center vertical-middle checked:bg-blue-500 checked:border-blue-500 checked:shadow-[0_0_20px_rgba(59,130,246,0.2)] hover:border-blue-500 hover:bg-zinc-800 focus:outline-none focus:shadow-[0_0_0_3px_rgba(59,130,246,0.2)] disabled:opacity-50 disabled:cursor-not-allowed"
             checked={modelFormData.enabled ?? true}
             onChange={(e) => setModelFormData({ ...modelFormData, enabled: e.target.checked })}
           />
           <label htmlFor="model-active" className="text-sm cursor-pointer">{t.ai.modelForm.active}</label>
         </div>
      </FormModal>

      <ConfirmationModal
        isOpen={!!channelToDelete}
        title={t.common.delete}
        message={t.ai.deleteChannelConfirm}
        onConfirm={confirmDeleteChannel}
        onCancel={() => setChannelToDelete(null)}
        type="danger"
      />

      <ConfirmationModal
        isOpen={!!modelToDelete}
        title={t.common.delete}
        message={t.ai.deleteModelConfirm}
        onConfirm={confirmDeleteModel}
        onCancel={() => setModelToDelete(null)}
        type="danger"
      />
    </div>
  );
};
