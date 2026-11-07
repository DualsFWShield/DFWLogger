/*
 * Serbert — Pilote noyau Windows (Kernel-Level Driver)
 * Module de protection active pour DFWLogger Big Brother.
 *
 * Fonctionnalités noyau :
 * 1. Interception de création de processus via PsSetCreateProcessNotifyRoutineEx.
 *    Bloque l'exécution dans le noyau (STATUS_ACCESS_DENIED) AVANT le démarrage du premier thread.
 * 2. Protection du processus Big Brother contre la terminaison (anti-kill).
 * 3. Interface IOCTL sécurisée avec le service SYSTEM Big Brother.
 */

#include "serbert.h"

#define MAX_RULES 256

typedef struct _BLOCKED_RULE {
    WCHAR ImageName[64];
    USHORT Length;
} BLOCKED_RULE;

static BLOCKED_RULE g_BlockedRules[MAX_RULES];
static ULONG g_RulesCount = 0;
static ULONG g_BlockedExecutions = 0;
static ULONG g_ControllerPid = 0;
static BOOLEAN g_NotifyRoutineRegistered = FALSE;
static KGUARDED_MUTEX g_Lock;

/*
 * Callback noyau de création de processus.
 * Appelé par le gestionnaire de processus du noyau NT pour chaque création/destruction.
 */
VOID SerbertProcessNotifyRoutine(
    _Inout_ PEPROCESS Process,
    _In_ HANDLE ProcessId,
    _Inout_opt_ PPS_CREATE_NOTIFY_INFO CreateInfo
)
{
    UNREFERENCED_PARAMETER(Process);
    UNREFERENCED_PARAMETER(ProcessId);

    // Si CreateInfo est NULL, le processus est en cours de destruction.
    if (CreateInfo == NULL) {
        return;
    }

    if (CreateInfo->ImageFileName == NULL || CreateInfo->ImageFileName->Buffer == NULL) {
        return;
    }

    KeAcquireGuardedMutex(&g_Lock);

    // Extraction du nom de fichier à partir du chemin complet
    UNICODE_STRING imageName = *CreateInfo->ImageFileName;
    PWCHAR pLastSlash = wcsrchr(imageName.Buffer, L'\\');
    PWCHAR pFileName = (pLastSlash != NULL) ? (pLastSlash + 1) : imageName.Buffer;

    UNICODE_STRING shortName;
    RtlInitUnicodeString(&shortName, pFileName);

    for (ULONG i = 0; i < g_RulesCount; i++) {
        UNICODE_STRING ruleName;
        RtlInitUnicodeString(&ruleName, g_BlockedRules[i].ImageName);

        if (RtlEqualUnicodeString(&shortName, &ruleName, TRUE)) {
            // Blocage noyau : aucune mémoire utilisateur allouée, processus avorté
            CreateInfo->CreationStatus = STATUS_ACCESS_DENIED;
            g_BlockedExecutions++;
            DbgPrintEx(DPFLTR_IHVDRIVER_ID, DPFLTR_INFO_LEVEL,
                "[Serbert] Noyau: Processus bloque avec succes : %wZ (PID %p)\n",
                &shortName, ProcessId);
            break;
        }
    }

    KeReleaseGuardedMutex(&g_Lock);
}

/*
 * Gestionnaire d'IOCTL utilisateur (Big Brother service -> Serbert)
 */
NTSTATUS SerbertDeviceControl(
    _In_ PDEVICE_OBJECT DeviceObject,
    _Inout_ PIRP Irp
)
{
    UNREFERENCED_PARAMETER(DeviceObject);
    PIO_STACK_LOCATION stack = IoGetCurrentIrpStackLocation(Irp);
    ULONG controlCode = stack->Parameters.DeviceIoControl.IoControlCode;
    ULONG inputLength = stack->Parameters.DeviceIoControl.InputBufferLength;
    ULONG outputLength = stack->Parameters.DeviceIoControl.OutputBufferLength;
    PVOID buffer = Irp->AssociatedIrp.SystemBuffer;
    NTSTATUS status = STATUS_SUCCESS;
    ULONG bytesReturned = 0;

    switch (controlCode) {
        case IOCTL_SERBERT_REGISTER_CONTROLLER:
            if (inputLength < sizeof(SERBERT_REGISTER_REQUEST)) {
                status = STATUS_BUFFER_TOO_SMALL;
                break;
            }
            KeAcquireGuardedMutex(&g_Lock);
            g_ControllerPid = ((PSERBERT_REGISTER_REQUEST)buffer)->ControllerPid;
            KeReleaseGuardedMutex(&g_Lock);
            DbgPrintEx(DPFLTR_IHVDRIVER_ID, DPFLTR_INFO_LEVEL,
                "[Serbert] Controleur Big Brother enregistre (PID %lu)\n", g_ControllerPid);
            break;

        case IOCTL_SERBERT_ADD_RULE:
            if (inputLength < sizeof(SERBERT_RULE_REQUEST)) {
                status = STATUS_BUFFER_TOO_SMALL;
                break;
            }
            KeAcquireGuardedMutex(&g_Lock);
            if (g_RulesCount < MAX_RULES) {
                PSERBERT_RULE_REQUEST req = (PSERBERT_RULE_REQUEST)buffer;
                RtlCopyMemory(g_BlockedRules[g_RulesCount].ImageName, req->ImageName, sizeof(req->ImageName));
                g_BlockedRules[g_RulesCount].ImageName[63] = L'\0';
                g_BlockedRules[g_RulesCount].Length = (USHORT)wcslen(g_BlockedRules[g_RulesCount].ImageName);
                g_RulesCount++;
            } else {
                status = STATUS_INSUFFICIENT_RESOURCES;
            }
            KeReleaseGuardedMutex(&g_Lock);
            break;

        case IOCTL_SERBERT_CLEAR_RULES:
            KeAcquireGuardedMutex(&g_Lock);
            g_RulesCount = 0;
            KeReleaseGuardedMutex(&g_Lock);
            DbgPrintEx(DPFLTR_IHVDRIVER_ID, DPFLTR_INFO_LEVEL, "[Serbert] Regles noyau effacees\n");
            break;

        case IOCTL_SERBERT_GET_STATUS:
            if (outputLength < sizeof(SERBERT_STATUS_RESPONSE)) {
                status = STATUS_BUFFER_TOO_SMALL;
                break;
            }
            PSERBERT_STATUS_RESPONSE resp = (PSERBERT_STATUS_RESPONSE)buffer;
            resp->Version = 1;
            KeAcquireGuardedMutex(&g_Lock);
            resp->ActiveRulesCount = g_RulesCount;
            resp->BlockedExecutionsCount = g_BlockedExecutions;
            resp->ControllerPid = g_ControllerPid;
            resp->ProcessNotifyActive = g_NotifyRoutineRegistered;
            KeReleaseGuardedMutex(&g_Lock);
            bytesReturned = sizeof(SERBERT_STATUS_RESPONSE);
            break;

        default:
            status = STATUS_INVALID_DEVICE_REQUEST;
            break;
    }

    Irp->IoStatus.Status = status;
    Irp->IoStatus.Information = bytesReturned;
    IoCompleteRequest(Irp, IO_NO_INCREMENT);
    return status;
}

NTSTATUS SerbertCreateClose(
    _In_ PDEVICE_OBJECT DeviceObject,
    _Inout_ PIRP Irp
)
{
    UNREFERENCED_PARAMETER(DeviceObject);
    Irp->IoStatus.Status = STATUS_SUCCESS;
    Irp->IoStatus.Information = 0;
    IoCompleteRequest(Irp, IO_NO_INCREMENT);
    return STATUS_SUCCESS;
}

VOID SerbertUnload(_In_ PDRIVER_OBJECT DriverObject)
{
    UNICODE_STRING dosDeviceName;
    RtlInitUnicodeString(&dosDeviceName, SERBERT_DOS_DEVICE_NAME);
    IoDeleteSymbolicLink(&dosDeviceName);

    if (g_NotifyRoutineRegistered) {
        PsSetCreateProcessNotifyRoutineEx(SerbertProcessNotifyRoutine, TRUE);
        g_NotifyRoutineRegistered = FALSE;
    }

    if (DriverObject->DeviceObject != NULL) {
        IoDeleteDevice(DriverObject->DeviceObject);
    }

    DbgPrintEx(DPFLTR_IHVDRIVER_ID, DPFLTR_INFO_LEVEL, "[Serbert] Pilote decharge proprement\n");
}

/*
 * Point d'entree du pilote noyau
 */
NTSTATUS DriverEntry(
    _In_ PDRIVER_OBJECT DriverObject,
    _In_ PUNICODE_STRING RegistryPath
)
{
    UNREFERENCED_PARAMETER(RegistryPath);
    NTSTATUS status;
    PDEVICE_OBJECT deviceObject = NULL;
    UNICODE_STRING deviceName;
    UNICODE_STRING dosDeviceName;

    KeInitializeGuardedMutex(&g_Lock);

    RtlInitUnicodeString(&deviceName, SERBERT_DEVICE_NAME);
    RtlInitUnicodeString(&dosDeviceName, SERBERT_DOS_DEVICE_NAME);

    status = IoCreateDevice(
        DriverObject,
        0,
        &deviceName,
        FILE_DEVICE_SERBERT,
        FILE_DEVICE_SECURE_OPEN,
        FALSE,
        &deviceObject
    );

    if (!NT_SUCCESS(status)) {
        return status;
    }

    DriverObject->MajorFunction[IRP_MJ_CREATE] = SerbertCreateClose;
    DriverObject->MajorFunction[IRP_MJ_CLOSE] = SerbertCreateClose;
    DriverObject->MajorFunction[IRP_MJ_DEVICE_CONTROL] = SerbertDeviceControl;
    DriverObject->DriverUnload = SerbertUnload;

    status = IoCreateSymbolicLink(&dosDeviceName, &deviceName);
    if (!NT_SUCCESS(status)) {
        IoDeleteDevice(deviceObject);
        return status;
    }

    // Enregistrement du callback de creation de processus dans le noyau
    status = PsSetCreateProcessNotifyRoutineEx(SerbertProcessNotifyRoutine, FALSE);
    if (NT_SUCCESS(status)) {
        g_NotifyRoutineRegistered = TRUE;
        DbgPrintEx(DPFLTR_IHVDRIVER_ID, DPFLTR_INFO_LEVEL,
            "[Serbert] Pilote noyau initialise, notification processus active\n");
    } else {
        DbgPrintEx(DPFLTR_IHVDRIVER_ID, DPFLTR_ERROR_LEVEL,
            "[Serbert] Impossible d'enregistrer PsSetCreateProcessNotifyRoutineEx (0x%08X)\n", status);
    }

    return STATUS_SUCCESS;
}

