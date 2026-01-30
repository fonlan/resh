import React, { useState, useRef, useEffect } from 'react';
import { createPortal } from 'react-dom';
import { Plus, Edit2, Trash2, Copy, Check, ExternalLink, Loader2 } from 'lucide-react';
import { AIChannel, AIModel, ProxyConfig } from '../../types/config';
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
  onAIChannelsUpdate: (channels: AIChannel[]) => void;
  onAIModelsUpdate: (models: AIModel[]) => void;
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
  onAIChannelsUpdate,
  onAIModelsUpdate,
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
      console.error("Failed to fetch models", e);
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
    <div className="tab-container">
      {/* AI Channels Section */}
      <div className="section-header flex justify-between items-center mb-4">
        <h3 className="section-title">{t.ai.channels}</h3>
        <button type="button" onClick={handleAddChannel} className="btn btn-primary">
          <Plus size={16} />
          {t.ai.addChannel}
        </button>
      </div>

      <div className="item-list mb-8">
        {sortedChannels.length === 0 ? (
          <div className="empty-state-mini">
            <p>{t.ai.noChannels}</p>
          </div>
        ) : (
          sortedChannels.map((channel) => (
            <div key={channel.id} className="item-card">
              <div className="item-info">
                <div className="flex items-center gap-2">
                  <span className="item-name">{channel.name}</span>
                  {channel.isActive && <span className="tag text-xs bg-green-900 text-green-200">Active</span>}
                </div>
                <p className="item-detail">Type: {channel.type}</p>
              </div>
              <div className="item-actions">
                <button
                  type="button"
                  onClick={() => handleEditChannel(channel)}
                  className="btn-icon btn-secondary"
                  title={t.common.edit}
                >
                  <Edit2 size={14} />
                </button>
                <button
                  type="button"
                  onClick={() => handleDeleteChannel(channel.id)}
                  className="btn-icon btn-secondary hover-danger"
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
      <div className="section-header flex justify-between items-center mb-4" style={{ marginTop: '32px' }}>
        <h3 className="section-title">{t.ai.models}</h3>
        <button type="button" onClick={handleAddModel} className="btn btn-primary">
          <Plus size={16} />
          {t.ai.addModel}
        </button>
      </div>

      <div className="item-list">
        {sortedModels.length === 0 ? (
          <div className="empty-state-mini">
            <p>{t.ai.noModels}</p>
          </div>
        ) : (
          sortedModels.map((model) => {
            const channel = aiChannels.find(c => c.id === model.channelId);
            const isChannelActive = channel ? channel.isActive : false;
            const effectivelyEnabled = model.enabled && isChannelActive;

            return (
              <div key={model.id} className={`item-card ${!effectivelyEnabled ? 'opacity-60' : ''}`}>
                <div className="item-info">
                  <div className="flex items-center gap-2">
                    <span className="item-name">{model.name}</span>
                    {!model.enabled && <span className="tag text-xs bg-gray-700 text-gray-300">Disabled</span>}
                    {model.enabled && !isChannelActive && (
                      <span className="tag text-xs bg-yellow-900 text-yellow-200" title="Parent channel is disabled">
                        Channel Disabled
                      </span>
                    )}
                  </div>
                  <p className="item-detail">Channel: {channel ? channel.name : 'Unknown Channel'}</p>
                </div>
                <div className="item-actions">
                  <button
                    type="button"
                    onClick={() => handleEditModel(model)}
                    className="btn-icon btn-secondary"
                    title={t.common.edit}
                  >
                    <Edit2 size={14} />
                  </button>
                  <button
                    type="button"
                    onClick={() => handleDeleteModel(model.id)}
                    className="btn-icon btn-secondary hover-danger"
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
              className="checkbox"
            />
            <label htmlFor="channel-synced" className="text-sm font-medium text-gray-300 cursor-pointer">
              {t.common.syncThisItem || 'Sync this item'}
            </label>
          </div>
        }
      >
        <div className="form-group">
          <label htmlFor="channel-name" className="form-label">{t.ai.channelForm.name}</label>
          <input
            id="channel-name"
            type="text"
            className="form-input"
            value={channelFormData.name || ''}
            onChange={(e) => setChannelFormData({ ...channelFormData, name: e.target.value })}
            placeholder="e.g. OpenAI Personal"
          />
        </div>
        <div className="form-group">
          <label htmlFor="channel-type" className="form-label">{t.ai.channelForm.type}</label>
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
           <div className="form-group p-4 bg-gray-800 rounded-lg border border-gray-700">
             <label className="form-label mb-2 block">GitHub Authentication</label>
             
             {channelFormData.apiKey ? (
               <div className="flex items-center gap-2 text-green-400 mb-4">
                 <Check size={18} />
                 <span>Authenticated with GitHub</span>
                 <button 
                   type="button" 
                   onClick={() => setChannelFormData({...channelFormData, apiKey: ''})}
                   className="text-xs text-gray-400 hover:text-white underline ml-2 bg-transparent border-none cursor-pointer p-0"
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
                     className="btn btn-secondary w-full flex items-center justify-center gap-2"
                   >
                     {isAuthLoading ? <Loader2 size={16} className="animate-spin" /> : null}
                     Sign in with GitHub
                   </button>
                 )}

                 {authError && (
                   <div className="text-red-400 text-sm mt-2">{authError}</div>
                 )}

                 {copilotAuthData && (
                   <div className="mt-4 space-y-4">
                     <div className="text-sm text-gray-300">
                       1. Copy code: <strong className="text-white select-all">{copilotAuthData.user_code}</strong>
                     </div>
                      <div className="text-sm text-gray-300">
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
                          className="btn btn-secondary flex-1 flex items-center justify-center gap-2"
                        >
                          <Copy size={14} /> Copy Code
                        </button>
                     </div>

                     <div className="text-center text-xs text-gray-400 flex items-center justify-center gap-2">
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
            <div className="form-group">
              <label htmlFor="channel-endpoint" className="form-label">{t.ai.channelForm.endpoint}</label>
              <input
                id="channel-endpoint"
                type="text"
                className="form-input"
                value={channelFormData.endpoint || ''}
                onChange={(e) => setChannelFormData({ ...channelFormData, endpoint: e.target.value })}
                placeholder="https://api.openai.com/v1"
              />
            </div>
            <div className="form-group">
              <label htmlFor="channel-apikey" className="form-label">{t.ai.channelForm.apiKey}</label>
              <input
                id="channel-apikey"
                type="password"
                className="form-input"
                value={channelFormData.apiKey || ''}
                onChange={(e) => setChannelFormData({ ...channelFormData, apiKey: e.target.value })}
                placeholder="sk-..."
              />
            </div>
          </>
        )}

        <div className="form-group flex items-center gap-2 mt-4">
          <input
            type="checkbox"
            id="channel-active"
            className="checkbox"
            checked={channelFormData.isActive ?? true}
            onChange={(e) => setChannelFormData({ ...channelFormData, isActive: e.target.checked })}
            disabled={channelFormData.type === 'copilot' && !channelFormData.apiKey}
          />
          <label htmlFor="channel-active" className="text-sm cursor-pointer">{t.ai.channelForm.active}</label>
        </div>

        <div className="form-group mt-4 pt-4 border-t border-gray-700">
          <label htmlFor="channel-proxy" className="form-label">{t.common.proxy}</label>
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
              className="checkbox"
            />
            <label htmlFor="model-synced" className="text-sm font-medium text-gray-300 cursor-pointer">
              {t.common.syncThisItem || 'Sync this item'}
            </label>
          </div>
        }
      >
          <div className="form-group relative">
            <label htmlFor="model-name" className="form-label">{t.ai.modelForm.name}</label>
            <div className="relative">
              <input
                id="model-name"
                ref={inputRef}
                type="text"
                className="form-input pr-8"
                value={modelFormData.name || ''}
                onChange={(e) => setModelFormData({ ...modelFormData, name: e.target.value })}
                placeholder="e.g. gpt-4, gpt-3.5-turbo"
                onFocus={() => {
                   handleFetchModels();
                   setShowModelSuggestions(true);
                }}
                onBlur={() => setTimeout(() => setShowModelSuggestions(false), 200)}
                autoComplete="off"
              />
              {isFetchingModels && (
                <div className="absolute right-2 top-1/2 transform -translate-y-1/2 pointer-events-none">
                  <Loader2 size={16} className="animate-spin text-gray-400" />
                </div>
              )}
              
              {showModelSuggestions && fetchedModels.length > 0 && dropdownPosition && createPortal(
                <ul 
                  className="snippet-group-suggestions fixed"
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
                      className="snippet-suggestion-item"
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
                     <li className="snippet-suggestion-item" style={{ cursor: 'default', opacity: 0.5 }}>
                        No matches found
                     </li>
                  )}
                </ul>,
                document.body
              )}
            </div>
          </div>
        <div className="form-group">
          <label htmlFor="model-channel" className="form-label">{t.ai.modelForm.channel}</label>
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

        <div className="form-group flex items-center gap-2 mt-4">
          <input
            type="checkbox"
            id="model-active"
            className="checkbox"
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
