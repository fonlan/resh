import React, { useState } from 'react';
import { Proxy } from '../../types/config';
import { validateRequired, validateUniqueName, validatePort } from '../../utils/validation';

interface ProxyFormProps {
  proxy?: Proxy;
  existingNames: string[];
  onSave: (proxy: Proxy) => void;
}

export const ProxyForm: React.FC<ProxyFormProps> = ({ proxy, existingNames, onSave }) => {
  const [formData, setFormData] = useState<Proxy>(
    proxy || {
      id: '',
      name: '',
      type: 'http',
      host: '',
      port: 8080,
      username: '',
      password: '',
    }
  );

  const [errors, setErrors] = useState<Record<string, string>>({});

  const validateForm = (): boolean => {
    const newErrors: Record<string, string> = {};

    const nameError = validateRequired(formData.name, 'Name');
    if (nameError) newErrors.name = nameError;

    const uniqueError = validateUniqueName(formData.name, existingNames, proxy?.name);
    if (uniqueError) newErrors.name = uniqueError;

    const hostError = validateRequired(formData.host, 'Host');
    if (hostError) newErrors.host = hostError;

    const portError = validatePort(formData.port, 'Port');
    if (portError) newErrors.port = portError;

    setErrors(newErrors);
    return Object.keys(newErrors).length === 0;
  };

  const handleSave = () => {
    if (validateForm()) {
      onSave(formData);
    }
  };

  const handleChange = (field: keyof Proxy, value: any) => {
    setFormData((prev) => ({
      ...prev,
      [field]: value,
    }));
    // Clear error for this field
    if (errors[field]) {
      setErrors((prev) => {
        const newErrors = { ...prev };
        delete newErrors[field];
        return newErrors;
      });
    }
  };

  return (
    <div className="space-y-4">
      {/* Name */}
      <div>
        <label className="block text-sm font-medium text-gray-300 mb-1">
          Proxy Name
        </label>
        <input
          type="text"
          value={formData.name}
          onChange={(e) => handleChange('name', e.target.value)}
          placeholder="e.g., Corporate Proxy, Home SOCKS5"
          className={`w-full px-3 py-2 rounded-md bg-gray-800 border ${
            errors.name ? 'border-red-500' : 'border-gray-600'
          } text-white placeholder-gray-500 focus:outline-none focus:ring-2 focus:ring-blue-500`}
        />
        {errors.name && <p className="text-red-400 text-xs mt-1">{errors.name}</p>}
      </div>

      {/* Type */}
      <div>
        <label className="block text-sm font-medium text-gray-300 mb-2">
          Proxy Type
        </label>
        <div className="flex gap-4">
          {(['http', 'socks5'] as const).map((type) => (
            <label key={type} className="flex items-center gap-2 cursor-pointer">
              <input
                type="radio"
                name="proxyType"
                value={type}
                checked={formData.type === type}
                onChange={() => handleChange('type', type)}
                className="w-4 h-4"
              />
              <span className="text-gray-300 uppercase">{type}</span>
            </label>
          ))}
        </div>
      </div>

      {/* Host */}
      <div>
        <label className="block text-sm font-medium text-gray-300 mb-1">
          Host
        </label>
        <input
          type="text"
          value={formData.host}
          onChange={(e) => handleChange('host', e.target.value)}
          placeholder="e.g., proxy.example.com"
          className={`w-full px-3 py-2 rounded-md bg-gray-800 border ${
            errors.host ? 'border-red-500' : 'border-gray-600'
          } text-white placeholder-gray-500 focus:outline-none focus:ring-2 focus:ring-blue-500`}
        />
        {errors.host && <p className="text-red-400 text-xs mt-1">{errors.host}</p>}
      </div>

      {/* Port */}
      <div>
        <label className="block text-sm font-medium text-gray-300 mb-1">
          Port
        </label>
        <input
          type="number"
          value={formData.port}
          onChange={(e) => handleChange('port', parseInt(e.target.value, 10))}
          min={1}
          max={65535}
          className={`w-full px-3 py-2 rounded-md bg-gray-800 border ${
            errors.port ? 'border-red-500' : 'border-gray-600'
          } text-white placeholder-gray-500 focus:outline-none focus:ring-2 focus:ring-blue-500`}
        />
        {errors.port && <p className="text-red-400 text-xs mt-1">{errors.port}</p>}
      </div>

      {/* Optional Credentials */}
      <div className="border-t border-gray-700 pt-4 mt-4">
        <h3 className="text-sm font-medium text-gray-300 mb-3">
          Authentication (Optional)
        </h3>

        <div className="space-y-3">
          {/* Username */}
          <div>
            <label className="block text-sm font-medium text-gray-400 mb-1">
              Username
            </label>
            <input
              type="text"
              value={formData.username || ''}
              onChange={(e) => handleChange('username', e.target.value)}
              placeholder="Leave empty if no auth required"
              className="w-full px-3 py-2 rounded-md bg-gray-800 border border-gray-600 text-white placeholder-gray-500 focus:outline-none focus:ring-2 focus:ring-blue-500"
            />
          </div>

          {/* Password */}
          <div>
            <label className="block text-sm font-medium text-gray-400 mb-1">
              Password
            </label>
            <input
              type="password"
              value={formData.password || ''}
              onChange={(e) => handleChange('password', e.target.value)}
              placeholder="Leave empty if no auth required"
              className="w-full px-3 py-2 rounded-md bg-gray-800 border border-gray-600 text-white placeholder-gray-500 focus:outline-none focus:ring-2 focus:ring-blue-500"
            />
          </div>
        </div>
      </div>

      {/* Save Button */}
      <div className="flex justify-end pt-4">
        <button
          onClick={handleSave}
          className="px-4 py-2 bg-blue-600 text-white rounded-md hover:bg-blue-700 focus:outline-none focus:ring-2 focus:ring-blue-500"
        >
          Save Proxy
        </button>
      </div>
    </div>
  );
};
