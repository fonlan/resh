import React, { useState, useRef, useEffect, useCallback } from 'react';
import { createPortal } from 'react-dom';
import { ChevronDown } from 'lucide-react';
import './CustomSelect.css';

export interface Option {
    value: string | number;
    label: string;
}

interface CustomSelectProps {
    value: string | number;
    onChange: (value: string) => void;
    options: Option[];
    placeholder?: string;
    disabled?: boolean;
    className?: string;
    id?: string;
    placement?: 'bottom' | 'top';
}

export const CustomSelect: React.FC<CustomSelectProps> = ({
    value,
    onChange,
    options,
    placeholder = "Select...",
    disabled = false,
    className = "",
    id,
    placement = 'bottom'
}) => {
    const [isOpen, setIsOpen] = useState(false);
    const [dropdownPosition, setDropdownPosition] = useState<{ top: number, left: number, width: number } | null>(null);
    const containerRef = useRef<HTMLDivElement>(null);

    const updatePosition = useCallback(() => {
        if (containerRef.current) {
            const rect = containerRef.current.getBoundingClientRect();
            const dropdownHeight = Math.min(options.length * 36 + 8, 300);
            const top = placement === 'top'
                ? rect.top - dropdownHeight - 4
                : rect.bottom + 4;
            setDropdownPosition({
                top,
                left: rect.left,
                width: rect.width
            });
        }
    }, [placement, options.length]);

    useEffect(() => {
        if (isOpen) {
            updatePosition();
            window.addEventListener('resize', updatePosition);
            window.addEventListener('scroll', updatePosition, true);
        }

        return () => {
            window.removeEventListener('resize', updatePosition);
            window.removeEventListener('scroll', updatePosition, true);
        };
    }, [isOpen, updatePosition]);

    // Handle click outside
    useEffect(() => {
        const handleClickOutside = (event: MouseEvent) => {
            if (!isOpen) return;
            
            const target = event.target as Node;
            const isInsideContainer = containerRef.current && containerRef.current.contains(target);
            
            if (!isInsideContainer) {
                setIsOpen(false);
            }
        };

        document.addEventListener('mousedown', handleClickOutside);

        return () => {
            document.removeEventListener('mousedown', handleClickOutside);
        };
    }, [isOpen]);

    const handleSelect = (optionValue: string | number, event: React.MouseEvent) => {
        event.stopPropagation();
        onChange(optionValue.toString());
        setIsOpen(false);
    };

    const selectedOption = options.find(opt => String(opt.value) === String(value));

    const dropdownContent = (
        <div 
            className="custom-select-dropdown"
            style={dropdownPosition ? {
                top: dropdownPosition.top,
                left: dropdownPosition.left,
                width: dropdownPosition.width
            } : undefined}
            role="listbox"
            onClick={(e) => e.stopPropagation()}
            onMouseDown={(e) => e.stopPropagation()}
            onMouseUp={(e) => e.stopPropagation()}
        >
            {options.length === 0 ? (
                <div className="custom-select-item" style={{ cursor: 'default', opacity: 0.5 }}>
                    No options
                </div>
            ) : (
                options.map((option) => (
                    <div
                        key={option.value}
                        className={`custom-select-item ${String(option.value) === String(value) ? 'selected' : ''}`}
                        onClick={(e) => handleSelect(option.value, e)}
                        onKeyDown={(e) => {
                            if (e.key === 'Enter' || e.key === ' ') {
                                e.preventDefault();
                                onChange(option.value.toString());
                                setIsOpen(false);
                            }
                        }}
                        role="option"
                        aria-selected={String(option.value) === String(value)}
                        tabIndex={0}
                        title={option.label}
                    >
                        {option.label}
                    </div>
                ))
            )}
        </div>
    );

    return (
        <div 
            ref={containerRef} 
            className={`custom-select-container ${className}`}
            id={id}
        >
            <div 
                className={`custom-select-trigger ${disabled ? 'disabled' : ''}`}
                onClick={() => !disabled && setIsOpen(!isOpen)}
                onKeyDown={(e) => {
                    if (disabled) return;
                    if (e.key === 'Enter' || e.key === ' ') {
                        e.preventDefault();
                        setIsOpen(!isOpen);
                    }
                }}
                role="combobox"
                aria-expanded={isOpen}
                aria-haspopup="listbox"
                tabIndex={disabled ? -1 : 0}
                title={selectedOption ? selectedOption.label : placeholder}
            >
                <span className={!selectedOption ? "text-gray-400" : ""}>
                    {selectedOption ? selectedOption.label : placeholder}
                </span>
                <ChevronDown size={16} className={`text-gray-400 transition-transform ${isOpen ? 'rotate-180' : ''}`} />
            </div>

            {isOpen && dropdownPosition && createPortal(
                dropdownContent,
                document.body
            )}
        </div>
    );
};
