import React, { useState, useRef, useEffect, useCallback } from 'react';
import { createPortal } from 'react-dom';
import { ChevronDown } from 'lucide-react';
import { EmojiText } from './EmojiText';

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
    triggerClassName?: string;
}

export const CustomSelect: React.FC<CustomSelectProps> = ({
    value,
    onChange,
    options,
    placeholder = "Select...",
    disabled = false,
    className = "",
    triggerClassName = "",
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
            className="fixed z-[9999] bg-[var(--bg-secondary)] border border-[var(--glass-border)] rounded-[var(--radius-md)] shadow-[0_10px_25px_rgba(0,0,0,0.5)] max-h-[200px] overflow-y-auto"
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
                <div className="w-full text-left px-3.5 py-2.5 bg-transparent border-0 text-[var(--text-primary)] text-[13px] cursor-default opacity-50 whitespace-nowrap overflow-hidden text-ellipsis">
                    No options
                </div>
            ) : (
                options.map((option) => (
                    <div
                        key={option.value}
                        className={`w-full text-left px-3.5 py-2.5 bg-transparent border-0 text-[var(--text-primary)] text-[13px] cursor-pointer transition-colors duration-200 block whitespace-nowrap overflow-hidden text-ellipsis hover:bg-[var(--bg-tertiary)] ${String(option.value) === String(value) ? '!text-[var(--accent-primary)] font-medium' : ''}`}
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
                        <EmojiText text={option.label} />
                    </div>
                ))
            )}
        </div>
    );

    return (
        <div
            ref={containerRef}
            className={`relative w-full ${className}`}
            id={id}
        >
            <div
                className={`w-full px-3 py-2.5 bg-[var(--bg-primary)] border border-[var(--glass-border)] rounded-[var(--radius-sm)] text-[var(--text-primary)] text-[13px] font-[var(--font-ui)]  leading-6 transition-all text-left flex justify-between items-center cursor-pointer outline-none focus:border-[var(--accent-primary)] focus:shadow-[0_0_0_2px_rgba(59,130,246,0.1)] ${disabled ? 'opacity-50 cursor-not-allowed pointer-events-none' : ''} ${triggerClassName}`}
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
                <span className={`whitespace-nowrap overflow-hidden text-ellipsis flex-1 min-w-0 mr-2 ${!selectedOption ? 'text-zinc-400' : ''}`}>
                    {selectedOption ? <EmojiText text={selectedOption.label} /> : placeholder}
                </span>
                <ChevronDown size={16} className="text-zinc-400 transition-transform duration-200" style={{ transform: isOpen ? 'rotate(180deg)' : 'none' }} />
            </div>

            {isOpen && dropdownPosition && createPortal(
                dropdownContent,
                document.body
            )}
        </div>
    );
};
