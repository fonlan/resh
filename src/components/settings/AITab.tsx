import React, { useState } from 'react';
import { Plus, Edit2, Trash2 } from 'lucide-react';
import { AIChannel, AIModel } from '../../types/config';
import { generateId } from '../../utils/idGenerator';
import { FormModal } from '../FormModal';
import { useTranslation } from '../../i18n';

interface AITabProps {
  aiChannels: AIChannel[];
  aiModels: AIModel[];
  onAIChannelsUpdate: (channels: AIChannel[]) => void;
  onAIModelsUpdate: (models: AIModel[]) => void;
}

export const AITab: React.FC<AITabProps> = ({
  aiChannels,
  aiModels,
  onAIChannelsUpdate,
  onAIModelsUpdate,
}) => {
  const { t } = useTranslation();
  // State for Channel Form
  const [isChannelFormOpen, setIsChannelFormOpen] = useState(false);
  const [editingChannel, setEditingChannel] = useState<AIChannel | null>(null);
  const [channelFormData, setChannelFormData] = useState<Partial<AIChannel>>({});

  // State for Model Form
  const [isModelFormOpen, setIsModelFormOpen] = useState(false);
  const [editingModel, setEditingModel] = useState<AIModel | null>(null);
  const [modelFormData, setModelFormData] = useState<Partial<AIModel>>({});

  // --- Channel Handlers ---

  const handleAddChannel = () => {
    setEditingChannel(null);
    setChannelFormData({
      name: '',
      type: 'openai',
      endpoint: '',
      apiKey: '',
      isActive: true,
    });
    setIsChannelFormOpen(true);
  };

  const handleEditChannel = (channel: AIChannel) => {
    setEditingChannel(channel);
    setChannelFormData({ ...channel });
    setIsChannelFormOpen(true);
  };

  const handleDeleteChannel = (id: string) => {
    if (window.confirm(t.ai.deleteChannelConfirm)) {
      onAIChannelsUpdate(aiChannels.filter((c) => c.id !== id));
      // Also cleanup models
      onAIModelsUpdate(aiModels.filter((m) => m.channelId !== id));
    }
  };

  const handleSaveChannel = () => {
    if (!channelFormData.name) return; // Simple validation

    const now = new Date().toISOString();

    if (editingChannel) {
      const updatedChannel: AIChannel = {
        ...editingChannel,
        name: channelFormData.name || 'New Channel',
        type: (channelFormData.type as 'openai' | 'copilot') || 'openai',
        endpoint: channelFormData.endpoint,
        apiKey: channelFormData.apiKey,
        isActive: channelFormData.isActive !== undefined ? channelFormData.isActive : true,
        updatedAt: now,
      };
      onAIChannelsUpdate(aiChannels.map((c) => (c.id === editingChannel.id ? updatedChannel : c)));
    } else {
      const newChannel: AIChannel = {
        id: generateId(),
        name: channelFormData.name || 'New Channel',
        type: (channelFormData.type as 'openai' | 'copilot') || 'openai',
        endpoint: channelFormData.endpoint,
        apiKey: channelFormData.apiKey,
        isActive: channelFormData.isActive !== undefined ? channelFormData.isActive : true,
        synced: true,
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
    });
    setIsModelFormOpen(true);
  };

  const handleEditModel = (model: AIModel) => {
    setEditingModel(model);
    setModelFormData({ ...model });
    setIsModelFormOpen(true);
  };

  const handleDeleteModel = (id: string) => {
    if (window.confirm(t.ai.deleteModelConfirm)) {
      onAIModelsUpdate(aiModels.filter((m) => m.id !== id));
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
        updatedAt: now,
      };
      onAIModelsUpdate(aiModels.map((m) => (m.id === editingModel.id ? updatedModel : m)));
    } else {
      const newModel: AIModel = {
        id: generateId(),
        name: modelFormData.name,
        channelId: modelFormData.channelId,
        synced: true,
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
        {aiChannels.length === 0 ? (
          <div className="empty-state-mini">
            <p>{t.ai.noChannels}</p>
          </div>
        ) : (
          aiChannels.map((channel) => (
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
      <div className="section-header flex justify-between items-center mb-4">
        <h3 className="section-title">{t.ai.models}</h3>
        <button type="button" onClick={handleAddModel} className="btn btn-primary">
          <Plus size={16} />
          {t.ai.addModel}
        </button>
      </div>

      <div className="item-list">
        {aiModels.length === 0 ? (
          <div className="empty-state-mini">
            <p>{t.ai.noModels}</p>
          </div>
        ) : (
          aiModels.map((model) => {
            const channel = aiChannels.find(c => c.id === model.channelId);
            return (
              <div key={model.id} className="item-card">
                <div className="item-info">
                  <span className="item-name">{model.name}</span>
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
          <select
            id="channel-type"
            className="form-select"
            value={channelFormData.type || 'openai'}
            onChange={(e) => setChannelFormData({ ...channelFormData, type: e.target.value as any })}
          >
            <option value="openai">OpenAI</option>
            <option value="copilot">GitHub Copilot</option>
          </select>
        </div>
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
        <div className="form-group flex items-center gap-2 mt-4">
          <input
            type="checkbox"
            id="channel-active"
            className="checkbox"
            checked={channelFormData.isActive ?? true}
            onChange={(e) => setChannelFormData({ ...channelFormData, isActive: e.target.checked })}
          />
          <label htmlFor="channel-active" className="text-sm cursor-pointer">{t.ai.channelForm.active}</label>
        </div>
      </FormModal>

      {/* Model Form Modal */}
      <FormModal
        isOpen={isModelFormOpen}
        title={editingModel ? t.ai.editModel : t.ai.addModel}
        onClose={() => setIsModelFormOpen(false)}
        onSubmit={handleSaveModel}
      >
        <div className="form-group">
          <label htmlFor="model-name" className="form-label">{t.ai.modelForm.name}</label>
          <input
            id="model-name"
            type="text"
            className="form-input"
            value={modelFormData.name || ''}
            onChange={(e) => setModelFormData({ ...modelFormData, name: e.target.value })}
            placeholder="e.g. gpt-4, gpt-3.5-turbo"
          />
        </div>
        <div className="form-group">
          <label htmlFor="model-channel" className="form-label">{t.ai.modelForm.channel}</label>
          <select
            id="model-channel"
            className="form-select"
            value={modelFormData.channelId || ''}
            onChange={(e) => setModelFormData({ ...modelFormData, channelId: e.target.value })}
          >
            {aiChannels.map(channel => (
              <option key={channel.id} value={channel.id}>
                {channel.name}
              </option>
            ))}
          </select>
        </div>
      </FormModal>
    </div>
  );
};
