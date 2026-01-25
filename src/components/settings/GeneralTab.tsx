import React from 'react';

export const GeneralTab: React.FC = () => {
  return (
    <div className="space-y-6">
      <div>
        <h3 className="text-lg font-semibold text-gray-900 mb-4">
          General Settings
        </h3>
        <div className="bg-gray-50 rounded-lg p-6 text-center text-gray-500">
          <p>General configuration options will appear here</p>
        </div>
      </div>
    </div>
  );
};
